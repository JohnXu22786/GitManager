//! Source-bound discovery and copied pair experience. The shared host owns
//! provider consent, worker fencing, renderer drafts, journal and final commit.
use super::change_adapter;
#[path = "discovery_flow/example.rs"]
mod example;
use crate::product_contract::*;
use crate::product_decisions::{
    accept_scene, AcceptedScene, Choice, DecisionEngine, IntentArchive, IntentionBinding,
    VerifiedChange,
};
use crate::product_discovery::{
    discover, DiscoveryPolicy, DiscoveryReport, PreparedDiscoveryCandidate, VerifiedRetainedHistory,
};
use crate::product_protocol::RuntimeView;
use crate::product_runtime::{LocalRuntime, ReplayAdmission};
use crate::product_store::{
    scope::{PreparedScopedChange, ScopedExecutionContext},
    ProductStore, ProjectSnapshot,
};
pub(super) use example::{ExampleExperience, ExamplePlayback};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}
fn check(store: &ProductStore, basis: &ProjectSnapshot, cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Err("This comparison was cancelled".into());
    }
    if store.load().map_err(error)? != *basis {
        return Err("Your saved work changed. Open a fresh comparison before choosing".into());
    }
    Ok(())
}
fn engine(store: &ProductStore, cancelled: Arc<AtomicBool>) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(
        LocalRuntime::with_cancellation(cancelled),
        IntentArchive::new(store.clone()),
    )
}
fn capture(
    request: &DevelopmentRequest,
    result: &DevelopmentResult,
    id: &str,
) -> Result<CapturedProgram> {
    let candidate = result
        .response
        .candidates
        .iter()
        .find(|c| c.id == id)
        .ok_or("That design is not in the returned result")?;
    CapturedProgram::capture(
        candidate.source_json.as_bytes(),
        &request.project_id,
        result.producer.clone(),
        None,
    )
    .map_err(error)
}

/// The shared host must preserve the existing scoped-rule route instead of
/// discarding a prepared layer's chosen population to fit whole-design play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AdmissionError {
    RuleComparisonRequired,
    Unavailable(String),
}
impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuleComparisonRequired => f.write_str("Keep this rule change in its existing comparison so the choice of which work it applies to is preserved"),
            Self::Unavailable(message) => f.write_str(message),
        }
    }
}
impl From<String> for AdmissionError {
    fn from(message: String) -> Self {
        Self::Unavailable(message)
    }
}
impl From<&str> for AdmissionError {
    fn from(message: &str) -> Self {
        Self::Unavailable(message.into())
    }
}
fn require_evolution(
    store: &ProductStore,
    basis: &ProjectSnapshot,
    prepared: &PreparedScopedChange,
    cancelled: &AtomicBool,
) -> std::result::Result<(), AdmissionError> {
    check(store, basis, cancelled)?;
    ScopedExecutionContext::prepared(basis, prepared).map_err(error)?;
    if prepared.initialization().is_some() {
        // Only new scoped layers carry this independently checked receipt.
        return Err(AdmissionError::RuleComparisonRequired);
    }
    if basis.editable_scope_context().map_err(error)?.is_none() {
        return Err("This prepared design needs a managed whole-design comparison".into());
    }
    // Regenerate through the public compiler rather than infer transition kind
    // from labels, producer prose, or the apparent observed effect.
    let mut rebuilt = store
        .prepare_managed_evolution(
            prepared.candidate(),
            &change_adapter::slot_mappings(basis, prepared.candidate())?,
            prepared.operation_id(),
        )
        .map_err(error)?;
    check(store, basis, cancelled)?;
    let mut original = prepared.clone();
    // Replay correspondences were checked above and do not change transition,
    // frozen population, compiler input or target identity.
    rebuilt.correspondences.clear();
    original.correspondences.clear();
    if rebuilt != original {
        return Err(
            "This design needs a compatible whole-design preparation before it can be explored"
                .into(),
        );
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct PreparedAlternative {
    pub id: Id,
    pub prepared: PreparedScopedChange,
    pub mappings: Vec<SemanticMapping>,
}
#[derive(Clone)]
struct Side {
    source: CapturedProgram,
    prepared: Option<PreparedScopedChange>,
}
#[derive(Clone)]
pub(super) struct DiscoveryDraft {
    basis: ProjectSnapshot,
    modify_request: DevelopmentRequest,
    modify_result: DevelopmentResult,
    request: DevelopmentRequest,
    primary: Side,
    need: String,
}
impl DiscoveryDraft {
    #[allow(clippy::too_many_arguments)]
    pub fn after_modify(
        store: &ProductStore,
        basis: &ProjectSnapshot,
        modify_request: DevelopmentRequest,
        modify_result: DevelopmentResult,
        candidate: &str,
        prepared: Option<PreparedScopedChange>,
        id: &str,
        need: &str,
        cancelled: Arc<AtomicBool>,
    ) -> std::result::Result<Self, AdmissionError> {
        check(store, basis, &cancelled)?;
        modify_request.validate().map_err(error)?;
        modify_result.validate_for(&modify_request).map_err(error)?;
        if modify_request.operation != DevelopmentOperation::Modify
            || modify_request.sources.last() != Some(basis.program().map_err(error)?)
            || modify_request.decisions != basis.decisions
            || modify_request.context.data_digest.as_ref()
                != Some(&basis.data.identity().map_err(error)?)
            || modify_request.context.session_digest.as_ref()
                != Some(&basis.session.identity().map_err(error)?)
            || id == modify_request.id
        {
            return Err("Discovery needs the exact completed change and a new request".into());
        }
        let authored = capture(&modify_request, &modify_result, candidate)?;
        let source = if let Some(p) = &prepared {
            if p.candidate() != &authored {
                return Err("The prepared design differs from the returned change".into());
            }
            require_evolution(store, basis, p, &cancelled)?;
            p.target().clone()
        } else {
            if basis.editable_scope_context().map_err(error)?.is_some() {
                return Err("Prepare this design against its saved rules before discovery".into());
            }
            authored
        };
        let request = engine(store, cancelled.clone())
            .inherit_request(
                basis,
                DevelopmentRequest {
                    version: CONTRACT_VERSION,
                    id: id.into(),
                    project_id: basis.data.project_id.clone(),
                    operation: DevelopmentOperation::Discover,
                    request: need.into(),
                    sources: vec![basis.program().map_err(error)?.clone(), source.clone()],
                    context: modify_request.context.clone(),
                    examples: vec![],
                    accepted_scenes: vec![],
                    decisions: basis.decisions.clone(),
                    unknowns: vec![],
                    required_capabilities: modify_request.required_capabilities.clone(),
                },
            )
            .map_err(error)?;
        check(store, basis, &cancelled)?;
        Ok(Self {
            basis: basis.clone(),
            modify_request,
            modify_result,
            request,
            primary: Side { source, prepared },
            need: need.into(),
        })
    }
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
    pub fn modify_request(&self) -> &DevelopmentRequest {
        &self.modify_request
    }
    pub fn modify_result(&self) -> &DevelopmentResult {
        &self.modify_result
    }
    pub fn primary_preparation(&self) -> Option<&PreparedScopedChange> {
        self.primary.prepared.as_ref()
    }

    /// Supported unchanged-slot route. Moved or ambiguous slots remain a repair
    /// request; no model-provided prose is treated as a compiler destination.
    pub fn prepare_alternatives(
        &self,
        store: &ProductStore,
        result: &DevelopmentResult,
        operation: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Vec<PreparedAlternative>> {
        check(store, &self.basis, &cancelled)?;
        result.validate_for(&self.request).map_err(error)?;
        if self
            .basis
            .editable_scope_context()
            .map_err(error)?
            .is_none()
        {
            return Ok(vec![]);
        }
        let mut prepared = vec![];
        for (index, candidate) in result.response.candidates.iter().enumerate() {
            check(store, &self.basis, &cancelled)?;
            if self
                .request
                .sources
                .iter()
                .any(|source| candidate.source_json.as_bytes() == source.source_bytes)
            {
                continue;
            }
            let authored = capture(&self.request, result, &candidate.id)?;
            let p = store
                .prepare_managed_evolution(
                    &authored,
                    &change_adapter::slot_mappings(&self.basis, &authored)?,
                    &format!("{operation}-{index}"),
                )
                .map_err(error)?;
            prepared.push(PreparedAlternative {
                id: candidate.id.clone(),
                prepared: p,
                mappings: vec![],
            });
        }
        check(store, &self.basis, &cancelled)?;
        Ok(prepared)
    }

    /// Policy is independently derived by the host before the provider result.
    /// Replace any cached history with freshly verified archive contents.
    pub fn evaluate(
        &self,
        store: &ProductStore,
        result: DevelopmentResult,
        mut policy: DiscoveryPolicy,
        alternatives: Vec<PreparedAlternative>,
        cancelled: Arc<AtomicBool>,
    ) -> std::result::Result<DiscoveryQueue, AdmissionError> {
        check(store, &self.basis, &cancelled)?;
        result.validate_for(&self.request).map_err(error)?;
        let mut history = VerifiedRetainedHistory::load(store).map_err(error)?;
        if let Some(p) = &self.primary.prepared {
            history
                .map_prepared_target(p.clone(), vec![])
                .map_err(error)?;
        } else {
            history
                .map_target(&self.primary.source, vec![])
                .map_err(error)?;
        }
        let mut sides = vec![
            Side {
                source: self.basis.program().map_err(error)?.clone(),
                prepared: None,
            },
            self.primary.clone(),
        ];
        let mut ids = BTreeSet::new();
        for alternative in alternatives {
            if !ids.insert(alternative.id.clone()) {
                return Err("A returned design was prepared twice".into());
            }
            require_evolution(store, &self.basis, &alternative.prepared, &cancelled)?;
            let checked = PreparedDiscoveryCandidate::from_result(
                &self.basis,
                &self.request,
                &result,
                &alternative.id,
                alternative.prepared.clone(),
                alternative.mappings,
            )
            .map_err(error)?;
            history.map_prepared_result(checked).map_err(error)?;
            sides.push(Side {
                source: alternative.prepared.target().clone(),
                prepared: Some(alternative.prepared),
            });
        }
        for candidate in &result.response.candidates {
            if !ids.contains(&candidate.id)
                && !self
                    .request
                    .sources
                    .iter()
                    .any(|source| candidate.source_json.as_bytes() == source.source_bytes)
            {
                let source = capture(&self.request, &result, &candidate.id)?;
                history.map_target(&source, vec![]).map_err(error)?;
                sides.push(Side {
                    source,
                    prepared: None,
                });
            }
        }
        policy.retained_history = Some(history);
        let report = discover(&self.request, &result, &policy, cancelled.clone()).map_err(error)?;
        check(store, &self.basis, &cancelled)?;
        Ok(DiscoveryQueue {
            basis: self.basis.clone(),
            report,
            sides,
            need: self.need.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueueState {
    Questions,
    NeedsRepair,
    Unverified,
    NoQuestions,
}
#[derive(Clone)]
pub(super) struct QuestionView {
    pub id: Id,
    pub statement: String,
    pub witnesses: Vec<ExampleView>,
    pub unknowns: Vec<String>,
}
#[derive(Clone)]
pub(super) struct ExampleView {
    pub labels: [String; 2],
    pub playable: bool,
    pub copyable: bool,
    pub example_issue: Option<String>,
    pub copy_issue: Option<String>,
}
#[derive(Clone)]
pub(super) struct QueueView {
    pub state: QueueState,
    pub questions: Vec<QuestionView>,
    pub defects: Vec<String>,
    pub unverified: Vec<String>,
    pub coverage: Vec<String>,
}
#[derive(Clone)]
pub(super) struct DiscoveryQueue {
    basis: ProjectSnapshot,
    report: DiscoveryReport,
    sides: Vec<Side>,
    need: String,
}
fn labels(basis: &ProjectSnapshot, sources: [&CapturedProgram; 2]) -> [String; 2] {
    let current = basis.program().ok();
    if sources.iter().any(|s| Some(*s) == current) {
        sources.map(|s| {
            if Some(s) == current {
                "Current"
            } else {
                "Alternative"
            }
            .into()
        })
    } else {
        ["Option A".into(), "Option B".into()]
    }
}
impl DiscoveryQueue {
    pub fn report(&self) -> &DiscoveryReport {
        &self.report
    }
    pub fn view(&self) -> QueueView {
        QueueView {
            state: if !self.report.defects.is_empty() {
                QueueState::NeedsRepair
            } else if !self.report.questions.is_empty() {
                QueueState::Questions
            } else if !self.report.unverified.is_empty() {
                QueueState::Unverified
            } else {
                QueueState::NoQuestions
            },
            questions: self
                .report
                .questions
                .iter()
                .map(|q| QuestionView {
                    id: q.id.clone(),
                    statement: q.statement.clone(),
                    witnesses: q
                        .witnesses
                        .iter()
                        .map(|w| ExampleView {
                            labels: labels(&self.basis, [w.before_program(), w.after_program()]),
                            playable: false,
                            copyable: false,
                            example_issue: None,
                            copy_issue: None,
                        })
                        .collect(),
                    unknowns: q.unknowns.clone(),
                })
                .collect(),
            defects: self.report.defects.clone(),
            unverified: self.report.unverified.clone(),
            coverage: self.report.coverage.clone(),
        }
    }
    /// Only enable controls whose real replay succeeded on this frozen basis.
    pub fn checked_view(
        &self,
        store: &ProductStore,
        cancelled: Arc<AtomicBool>,
    ) -> Result<QueueView> {
        check(store, &self.basis, &cancelled)?;
        let mut view = self.view();
        for question in &mut view.questions {
            for (index, example) in question.witnesses.iter_mut().enumerate() {
                match self.open_example(store, &question.id, index, cancelled.clone()) {
                    Ok(_) => example.playable = true,
                    Err(issue) => example.example_issue = Some(issue),
                }
                match self.open_pair(store, &question.id, index, cancelled.clone()) {
                    Ok(_) => example.copyable = true,
                    Err(issue) => example.copy_issue = Some(issue),
                }
            }
        }
        check(store, &self.basis, &cancelled)?;
        Ok(view)
    }
    pub fn open_example(
        &self,
        store: &ProductStore,
        question: &str,
        witness: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ExampleExperience> {
        check(store, &self.basis, &cancelled)?;
        let q = self
            .report
            .questions
            .iter()
            .find(|q| q.id == question)
            .ok_or("That question is no longer available")?;
        let w = q
            .witnesses
            .get(witness)
            .ok_or("That example is no longer available")?;
        let side = |source: &CapturedProgram| {
            self.sides
                .iter()
                .find(|s| &s.source == source)
                .cloned()
                .unwrap_or(Side {
                    source: source.clone(),
                    prepared: None,
                })
        };
        ExampleExperience::new(
            store,
            self.basis.clone(),
            [side(w.before_program()), side(w.after_program())],
            w.clone(),
            cancelled,
        )
    }
    pub fn open_pair(
        &self,
        store: &ProductStore,
        question: &str,
        witness: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<PairExperience> {
        check(store, &self.basis, &cancelled)?;
        let q = self
            .report
            .questions
            .iter()
            .find(|q| q.id == question)
            .ok_or("That question is no longer available")?;
        let w = q
            .witnesses
            .get(witness)
            .ok_or("That example is no longer available")?;
        let sources = [w.before_program(), w.after_program()];
        let mut sides = vec![];
        for source in sources {
            sides.push(self.sides.iter().find(|s| &s.source == source).cloned()
                .ok_or("This historical example needs a freshly prepared design before it can be changed")?);
        }
        let pair = [sides.remove(0), sides.remove(0)];
        // A pure-observation minimum remains playable as the exact example,
        // but recording needs an actually exercised business operation.
        let mut scenario = if experienced_operations(&pair, &w.witness().scenario).is_empty() {
            w.initial_scenario().clone()
        } else {
            w.witness().scenario.clone()
        };
        scenario.seed = self.basis.data.clone();
        scenario.clock_day = self.basis.clock_day;
        scenario.session = shared_session(&self.basis, &pair)?;
        let minimal = scenario == w.witness().scenario
            && w.witness()
                .minimization
                .as_ref()
                .is_some_and(|m| m.complete);
        let operations = experienced_operations(&pair, &scenario);
        PairExperience::new(
            store,
            self.basis.clone(),
            pair,
            scenario,
            self.need.clone(),
            DecisionScope {
                operations,
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            minimal,
            cancelled,
        )
    }
}

// Source-qualified actions of the exact admitted input, not a hypothesis's
// grouping label. Form and control bindings can name different action IDs.
fn experienced_operations(sides: &[Side; 2], scenario: &ScenarioSpec) -> BTreeSet<Id> {
    sides
        .iter()
        .flat_map(|side| {
            scenario.inputs.iter().filter_map(|input| {
                let app = &side.source.program;
                match input {
                    SemanticInput::Invoke { action, .. } => Some(action.clone()),
                    SemanticInput::Submit { view, .. } => {
                        app.views.iter().find(|v| &v.id == view).and_then(|v| {
                            if let ViewKind::Form { action, .. } = &v.kind {
                                Some(action.clone())
                            } else {
                                None
                            }
                        })
                    }
                    SemanticInput::Activate { view, binding, .. } => app
                        .views
                        .iter()
                        .find(|v| &v.id == view)
                        .and_then(|v| v.actions.iter().find(|a| &a.id == binding))
                        .map(|a| a.action.clone()),
                    SemanticInput::Control { view, control, .. } => {
                        app.views.iter().find(|v| &v.id == view).and_then(|v| {
                            if let ViewKind::List {
                                controls,
                                selection,
                                ..
                            } = &v.kind
                            {
                                controls
                                    .iter()
                                    .find(|c| &c.id == control)
                                    .and_then(|c| c.on_change.clone())
                                    .or_else(|| {
                                        selection
                                            .as_ref()
                                            .filter(|s| &s.id == control)
                                            .and_then(|s| s.on_change.clone())
                                    })
                            } else {
                                None
                            }
                        })
                    }
                    _ => None,
                }
            })
        })
        .collect()
}

fn shared_session(basis: &ProjectSnapshot, sides: &[Side; 2]) -> Result<SessionState> {
    let mut shared = basis.session.clone();
    for side in sides {
        for state in &side.source.program.state {
            if !shared.values.contains_key(&state.id) {
                shared
                    .values
                    .insert(state.id.clone(), state.initial.clone());
            }
        }
    }
    for side in sides {
        shared.validate(&side.source.program).map_err(|_| "These designs need a compatible shared working state before they can be tried together".to_string())?;
    }
    Ok(shared)
}
#[derive(Clone)]
pub(super) struct PairView {
    pub labels: [String; 2],
    pub runs: [RuntimeView; 2],
    pub ticket: Digest,
    pub minimal: bool,
    pub ready: bool,
    pub available: [bool; 6],
    pub readiness_notes: Vec<String>,
    pub actions: Vec<String>,
    pub trial_day: i32,
}
#[derive(Clone)]
pub(super) struct PairExperience {
    basis: ProjectSnapshot,
    sides: [Side; 2],
    scenario: ScenarioSpec,
    need: String,
    scope: DecisionScope,
    minimal: bool,
    views: [RuntimeView; 2],
    scenes: Option<[AcceptedScene; 2]>,
    trial_day: i32,
    trial_observation: Option<Id>,
}
impl PairExperience {
    #[allow(clippy::too_many_arguments)]
    fn new(
        store: &ProductStore,
        basis: ProjectSnapshot,
        sides: [Side; 2],
        scenario: ScenarioSpec,
        need: String,
        scope: DecisionScope,
        minimal: bool,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        let (views, scenes, trial_day) = replay_pair(store, &basis, &sides, &scenario, cancelled)?;
        Ok(Self {
            basis,
            sides,
            scenario,
            need,
            scope,
            minimal,
            views,
            scenes: Some(scenes),
            trial_day,
            trial_observation: None,
        })
    }
    pub fn view(&self) -> PairView {
        PairView {
            labels: labels(&self.basis, [&self.sides[0].source, &self.sides[1].source]),
            runs: self.views.clone(),
            ticket: self.ticket().expect("checked pair"),
            minimal: self.minimal,
            ready: self.scenes.is_some(),
            available: [false; 6],
            readiness_notes: vec![],
            actions: experienced_operations(&self.sides, &self.scenario)
                .iter()
                .map(|id| {
                    self.sides
                        .iter()
                        .flat_map(|side| &side.source.program.actions)
                        .find(|action| &action.id == id)
                        .map(|action| action.label.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect(),
            trial_day: self.trial_day,
        }
    }
    fn ticket(&self) -> Result<Digest> {
        canonical_digest(
            IdentityDomain::Evidence,
            &(
                &self.basis,
                &self.sides[0].source,
                &self.sides[1].source,
                &self.scenario,
            ),
        )
        .map_err(error)
    }
    /// Readiness is checked on the worker. The ordinary view deliberately has
    /// no enabled choice buttons until these exact backend preparations pass.
    pub fn checked_view(
        &self,
        store: &ProductStore,
        operation: &str,
        resolves: &[Id],
        cancelled: Arc<AtomicBool>,
    ) -> Result<PairView> {
        check(store, &self.basis, &cancelled)?;
        let mut view = self.view();
        for (index, outcome) in self.outcomes().into_iter().enumerate() {
            match self.prepare_choice(
                store,
                &view.ticket,
                outcome,
                &format!("{operation}-choice-{index}"),
                &format!("{operation}-{index}"),
                resolves,
                cancelled.clone(),
            ) {
                Ok(_) => view.available[index] = true,
                Err(note) => view.readiness_notes.push(note),
            }
        }
        check(store, &self.basis, &cancelled)?;
        Ok(view)
    }
    fn outcomes(&self) -> [DecisionOutcome; 6] {
        [
            DecisionOutcome::Accept {
                artifact: self.sides[0].source.artifact.program_digest.clone(),
            },
            DecisionOutcome::Accept {
                artifact: self.sides[1].source.artifact.program_digest.clone(),
            },
            DecisionOutcome::EitherAcceptable,
            DecisionOutcome::BothNeeded,
            DecisionOutcome::NeitherFits,
            DecisionOutcome::Deferred,
        ]
    }
    pub fn scenes(&self) -> Option<&[AcceptedScene; 2]> {
        self.scenes.as_ref()
    }
    pub fn trial(
        &mut self,
        store: &ProductStore,
        input: SemanticInput,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        // A failed or cancelled new input must not leave old choice authority.
        self.scenes = None;
        self.minimal = false;
        check(store, &self.basis, &cancelled)?;
        let mut next = self.scenario.clone();
        if let Some(point) = &self.trial_observation {
            if matches!(next.inputs.last(), Some(SemanticInput::Observe {point:last}) if last == point)
            {
                next.inputs.pop();
            }
        }
        next.inputs.push(input);
        let mut ordinal = next.inputs.len();
        let point = loop {
            let candidate = format!("copied-outcome-{ordinal}");
            if !next
                .inputs
                .iter()
                .any(|i| matches!(i, SemanticInput::Observe {point} if point == &candidate))
            {
                break candidate;
            }
            ordinal += 1;
        };
        next.inputs.push(SemanticInput::Observe {
            point: point.clone(),
        });
        let (views, scenes, day) = replay_pair(store, &self.basis, &self.sides, &next, cancelled)?;
        self.scenario = next;
        self.views = views;
        self.scenes = Some(scenes);
        self.trial_day = day;
        self.trial_observation = Some(point);
        Ok(())
    }
    /// Reopen an exact saved two-design need on today's saved work, without
    /// reviving its old preparations or silently replacing its business IDs.
    pub fn reopen(
        store: &ProductStore,
        decision: &str,
        operation: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        let basis = store.load().map_err(error)?;
        let d = basis
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == decision && d.status == DecisionStatus::Pending)
            .ok_or("This pending choice is no longer available")?;
        let saved = engine(store, cancelled.clone())
            .discovery_scenes(&basis)
            .map_err(error)?;
        let scenes: Vec<_> = saved.iter().filter(|s| s.decision() == decision).collect();
        if scenes.len() != 2 || scenes[0].scenario() != scenes[1].scenario() {
            return Err(
                "This need requires a new design comparison before it can be reopened".into(),
            );
        }
        let mut sides = vec![];
        for (index, scene) in scenes.iter().enumerate() {
            let source = basis
                .programs
                .iter()
                .find(|p| {
                    p.binding == scene.original().binding.source
                        && p.artifact == scene.original().binding.artifact
                })
                .ok_or("The saved design source is missing")?;
            let source_id = canonical_digest(IdentityDomain::Source, source).map_err(error)?;
            let proof = basis
                .scope
                .rehearsals
                .get(&source_id)
                .ok_or("This saved comparison is not a managed design pair")?;
            let authored = basis
                .programs
                .iter()
                .find(|p| {
                    canonical_digest(IdentityDomain::Source, p).ok().as_ref()
                        == Some(&proof.manifest.business)
                })
                .ok_or("The saved editable design is missing")?;
            let p = store
                .prepare_managed_evolution(
                    authored,
                    &change_adapter::slot_mappings(&basis, authored)?,
                    &format!("{operation}-{index}"),
                )
                .map_err(error)?;
            sides.push(Side {
                source: p.target().clone(),
                prepared: Some(p),
            });
        }
        let pair = [sides.remove(0), sides.remove(0)];
        let mut scenario = scenes[0].scenario().clone();
        scenario.seed = basis.data.clone();
        scenario.clock_day = basis.clock_day;
        scenario.session = shared_session(&basis, &pair)?;
        let need = d.request.clone();
        let scope = d.scope.clone();
        Self::new(store, basis, pair, scenario, need, scope, false, cancelled)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_choice(
        &self,
        store: &ProductStore,
        ticket: &Digest,
        outcome: DecisionOutcome,
        decision: &str,
        operation: &str,
        resolves: &[Id],
        cancelled: Arc<AtomicBool>,
    ) -> Result<VerifiedChange> {
        check(store, &self.basis, &cancelled)?;
        if &self.ticket()? != ticket || self.scenes.is_none() {
            return Err("Experience the latest copied work before choosing".into());
        }
        // Backend preparation independently reproduces these exact accepted
        // scenes. Do not redundantly rerun both renderers for each button check.
        let scenes = self.scenes.as_ref().expect("checked scenes");
        let checker = engine(store, cancelled.clone());
        let mut choice = Choice {
            id: decision.into(),
            request: self.need.clone(),
            rationale: None,
            scope: DecisionScope {
                operations: experienced_operations(&self.sides, &self.scenario),
                ..self.scope.clone()
            },
            outcome: outcome.clone(),
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let selected = match &outcome {
            DecisionOutcome::Accept { artifact } => Some(
                self.sides
                    .iter()
                    .position(|s| &s.source.artifact.program_digest == artifact)
                    .ok_or("Choose one of the designs actually experienced")?,
            ),
            DecisionOutcome::KeepCurrent => Some(
                self.sides
                    .iter()
                    .position(|s| self.basis.program().ok() == Some(&s.source))
                    .ok_or("Neither displayed option is the current tool")?,
            ),
            _ => None,
        };
        let result = if let Some(index) = selected {
            if let Some(prior) = &self.sides[index].prepared {
                // Selected adoption is separately prepared with its final operation
                // ID; paired nonbinary recording never installs a temporary side.
                let fresh = store
                    .prepare_managed_evolution(
                        prior.candidate(),
                        &change_adapter::slot_mappings(&self.basis, prior.candidate())?,
                        operation,
                    )
                    .map_err(error)?;
                choice.scope = fresh.scope().clone();
                choice.outcome = DecisionOutcome::Accept {
                    artifact: fresh.target().artifact.program_digest.clone(),
                };
                let scene = checker
                    .accept_scoped_scene(
                        store,
                        &fresh,
                        &self.scenario,
                        Disclosure::ExplicitlySelected,
                    )
                    .map_err(error)?;
                if scene.observations() != scenes[index].observations() {
                    return Err(
                        "The selected design changed while it was being prepared. Try it again"
                            .into(),
                    );
                }
                if resolves.is_empty() {
                    checker.prepare_scoped_choice(store, fresh, choice, vec![scene], operation)
                } else {
                    checker.prepare_scoped_resolution(
                        store,
                        fresh,
                        choice,
                        vec![scene],
                        resolves,
                        operation,
                    )
                }
            } else if resolves.is_empty() {
                checker.prepare_choice(
                    store,
                    &self.sides[index].source,
                    choice,
                    vec![scenes[index].clone()],
                    operation,
                )
            } else {
                checker.prepare_resolution(
                    store,
                    &self.sides[index].source,
                    choice,
                    vec![scenes[index].clone()],
                    resolves,
                    operation,
                )
            }
        } else {
            match (&self.sides[0].prepared, &self.sides[1].prepared) {
                (Some(a), Some(b)) => checker.prepare_paired_rehearsed_choice(
                    store,
                    a.clone(),
                    b.clone(),
                    choice,
                    scenes.to_vec(),
                    resolves,
                    operation,
                ),
                (Some(p), None) | (None, Some(p)) => checker.prepare_rehearsed_choice(
                    store,
                    p.clone(),
                    choice,
                    scenes.to_vec(),
                    resolves,
                    operation,
                ),
                (None, None) if resolves.is_empty() => checker.prepare_choice(
                    store,
                    self.basis.program().map_err(error)?,
                    choice,
                    scenes.to_vec(),
                    operation,
                ),
                (None, None) => checker.prepare_resolution(
                    store,
                    self.basis.program().map_err(error)?,
                    choice,
                    scenes.to_vec(),
                    resolves,
                    operation,
                ),
            }
        }
        .map_err(error)?;
        check(store, &self.basis, &cancelled)?;
        Ok(result)
    }
}
fn replay_pair(
    store: &ProductStore,
    basis: &ProjectSnapshot,
    sides: &[Side; 2],
    scenario: &ScenarioSpec,
    cancelled: Arc<AtomicBool>,
) -> Result<([RuntimeView; 2], [AcceptedScene; 2], i32)> {
    check(store, basis, &cancelled)?;
    let context = pair_context(basis, sides)?;
    let checker = engine(store, cancelled.clone());
    let scenes = if let (Some(a), Some(b)) = (&sides[0].prepared, &sides[1].prepared) {
        checker
            .accept_paired_scoped_scenes(store, a, b, scenario, Disclosure::ExplicitlySelected)
            .map_err(error)?
    } else {
        let mut scenes = vec![];
        for side in sides {
            scenes.push(
                if let Some(p) = &side.prepared {
                    checker.accept_scoped_scene(store, p, scenario, Disclosure::ExplicitlySelected)
                } else if side.source == *basis.program().map_err(error)? {
                    if let Some(p) = sides.iter().find_map(|s| s.prepared.as_ref()) {
                        checker.accept_prepared_current_scene(
                            store,
                            p,
                            scenario,
                            Disclosure::ExplicitlySelected,
                        )
                    } else {
                        checker.accept_current_scene(
                            basis,
                            scenario,
                            Disclosure::ExplicitlySelected,
                        )
                    }
                } else {
                    accept_scene(
                        &LocalRuntime::with_cancellation(cancelled.clone()),
                        &side.source,
                        scenario,
                        Disclosure::ExplicitlySelected,
                        RuntimeLimits::default(),
                    )
                }
                .map_err(error)?,
            );
        }
        [scenes.remove(0), scenes.remove(0)]
    };
    let (views, day) = run_views(store, basis, sides, scenario, &context, cancelled)?;
    Ok((views, scenes, day))
}
fn pair_context(basis: &ProjectSnapshot, sides: &[Side; 2]) -> Result<ScopedExecutionContext> {
    match (&sides[0].prepared, &sides[1].prepared) {
        (Some(a), Some(b)) => ScopedExecutionContext::rehearsed_pair(basis, a, b),
        (Some(p), None) | (None, Some(p)) => ScopedExecutionContext::prepared(basis, p),
        (None, None) => ScopedExecutionContext::committed(basis),
    }
    .map_err(error)
}
fn run_views(
    store: &ProductStore,
    basis: &ProjectSnapshot,
    sides: &[Side; 2],
    scenario: &ScenarioSpec,
    context: &ScopedExecutionContext,
    cancelled: Arc<AtomicBool>,
) -> Result<([RuntimeView; 2], i32)> {
    check(store, basis, &cancelled)?;
    let runtime = LocalRuntime::with_cancellation(cancelled.clone())
        .with_admission(Arc::new(context.clone()));
    let mut views = vec![];
    let mut days = vec![];
    let mut namespaces = vec![];
    for side in sides {
        context
            .validate_seed(&side.source, &scenario.seed, scenario.clock_day)
            .map_err(error)?;
        let mut run = runtime
            .start(
                &side.source,
                &scenario.seed,
                &scenario.session,
                scenario.clock_day,
                scenario.random_seed,
                RuntimeLimits::default(),
            )
            .map_err(error)?;
        let operations = context
            .replay_operation_ids(&side.source, scenario)
            .map_err(error)?;
        namespaces.push(operations.clone());
        for (input, operation) in scenario.inputs.iter().zip(operations) {
            runtime.apply(&mut run, input, &operation).map_err(error)?;
            context
                .validate_state(&side.source, runtime.data(&run), run.clock_day())
                .map_err(error)?;
        }
        views.push(runtime.view_model(&run).map_err(error)?);
        days.push(run.clock_day());
    }
    if namespaces[0] != namespaces[1] || days[0] != days[1] {
        return Err("The two copied runs did not use the same input sequence".into());
    }
    check(store, basis, &cancelled)?;
    Ok(([views.remove(0), views.remove(0)], days[0]))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    OpenExample {
        question: Id,
        witness: usize,
    },
    OpenCopies {
        question: Id,
        witness: usize,
    },
    Choose {
        ticket: Digest,
        outcome: DecisionOutcome,
    },
    Return,
}
/// Workflow controls only. The host embeds its two existing runtime renderers
/// with the PairView runs and routes their semantic input through trial.
pub(super) fn show_queue(ui: &mut egui::Ui, view: &QueueView) -> Option<Event> {
    ui.heading("Explore the choices");
    ui.label(match view.state {
        QueueState::Questions => "Try an example before deciding what matters to you",
        QueueState::NeedsRepair => "A saved requirement needs repair before you can choose",
        QueueState::Unverified => "Some behavior could not be checked. No preference is assumed",
        QueueState::NoQuestions => "No unresolved choice was found in the checked examples",
    });
    for text in view.defects.iter().chain(&view.unverified) {
        ui.label(text);
    }
    let mut event = None;
    for question in &view.questions {
        ui.group(|ui| {
            ui.label(&question.statement);
            for unknown in &question.unknowns {
                ui.label(unknown);
            }
            for (witness, example) in question.witnesses.iter().enumerate() {
                ui.label(format!("{} / {}", example.labels[0], example.labels[1]));
                if ui
                    .add_enabled(
                        example.playable,
                        egui::Button::new(format!("Try example {}", witness + 1)),
                    )
                    .clicked()
                {
                    event = Some(Event::OpenExample {
                        question: question.id.clone(),
                        witness,
                    });
                }
                if ui
                    .add_enabled(example.copyable, egui::Button::new("Try on my saved work"))
                    .clicked()
                {
                    event = Some(Event::OpenCopies {
                        question: question.id.clone(),
                        witness,
                    });
                }
                if let Some(issue) = &example.example_issue {
                    ui.label(format!("This example cannot be replayed here: {issue}"));
                }
                if let Some(issue) = &example.copy_issue {
                    ui.label(format!(
                        "This example cannot be applied to the current work: {issue}"
                    ));
                }
            }
        });
    }
    ui.collapsing("What was checked", |ui| {
        for text in &view.coverage {
            ui.label(text);
        }
    });
    if ui.button("Return to saved work").clicked() {
        event = Some(Event::Return);
    }
    event
}
pub(super) fn show_choices(
    ui: &mut egui::Ui,
    view: &PairView,
    blocked_by_edits: bool,
) -> Option<Event> {
    ui.label("Both versions use copies of your saved work. Changes here are not saved");
    ui.label(format!(
        "Actions in this copied example: {}",
        view.actions.join(", ")
    ));
    ui.label("Saved rules and work are checked before a design can be chosen");
    if view.minimal {
        ui.label("Every permitted single simplification of this example was checked");
    }
    if !view.readiness_notes.is_empty() {
        ui.collapsing("Why some choices are unavailable", |ui| {
            for note in view.readiness_notes.iter().collect::<BTreeSet<_>>() {
                ui.label(note);
            }
        });
    }
    let mut event = None;
    ui.add_enabled_ui(view.ready && !blocked_by_edits, |ui| {
        for (index, (label, outcome)) in [
            (
                format!("Choose {}", view.labels[0]),
                DecisionOutcome::Accept {
                    artifact: view.runs[0].program.identity().expect("checked program"),
                },
            ),
            (
                format!("Choose {}", view.labels[1]),
                DecisionOutcome::Accept {
                    artifact: view.runs[1].program.identity().expect("checked program"),
                },
            ),
            ("Either works".into(), DecisionOutcome::EitherAcceptable),
            ("I need both".into(), DecisionOutcome::BothNeeded),
            ("Neither fits".into(), DecisionOutcome::NeitherFits),
            ("Decide later".into(), DecisionOutcome::Deferred),
        ]
        .into_iter()
        .enumerate()
        {
            if ui
                .add_enabled(view.available[index], egui::Button::new(label))
                .clicked()
            {
                event = Some(Event::Choose {
                    ticket: view.ticket.clone(),
                    outcome,
                });
            }
        }
    });
    if ui.button("Discard copied edits and return").clicked() {
        event = Some(Event::Return);
    }
    event
}
