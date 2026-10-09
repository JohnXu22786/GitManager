//! Fixed-population current-versus-one-rule discovery. This is replay and trace
//! preparation only; the existing ChangeDraft, worker and journal own choices.
use super::discovery_flow::{ExamplePlayback, ExampleView, QuestionView, QueueState, QueueView};
use crate::product_contract::*;
use crate::product_decisions::{DecisionEngine, IntentArchive};
use crate::product_discovery::{
    discover, CheckedWitnessReplay, DiscoveryPolicy, DiscoveryReport, PreparedDiscoveryCandidate,
    VerifiedRetainedHistory,
};
use crate::product_protocol::RuntimeView;
use crate::product_runtime::{LocalRuntime, ReplayAdmission};
use crate::product_scenarios::VerifiedWitness;
use crate::product_store::{
    scope::{PreparedScopedChange, ScopeRequest, ScopedExecutionContext},
    ProductStore, ProjectSnapshot,
};
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
        return Err("This rule comparison was cancelled".into());
    }
    if store.load().map_err(error)? != *basis {
        return Err("Saved work changed. Open a fresh rule comparison".into());
    }
    Ok(())
}

/// The exact business selection, including lifecycle meaning and operation
/// identity. Identical saved data does not make two different selections equal.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct RuleSelection {
    basis: ProjectSnapshot,
    prepared: PreparedScopedChange,
    request: ScopeRequest,
    operation: Id,
}
impl RuleSelection {
    pub fn checked(
        store: &ProductStore,
        basis: &ProjectSnapshot,
        prepared: PreparedScopedChange,
        request: ScopeRequest,
        operation: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        check(store, basis, &cancelled)?;
        if request
            .patches
            .iter()
            .any(|p| !request.lifecycles.iter().any(|l| l.entity == p.entity))
        {
            return Err("Show when this work counts as finished, or explicitly say it has no finished state".into());
        }
        if request.operations.is_empty() {
            return Err("Try a business task before discovering this rule's choices".into());
        }
        ScopedExecutionContext::prepared(basis, &prepared).map_err(error)?;
        if prepared.initialization().is_none() || prepared.layer_id().map_err(error)?.is_none() {
            return Err("This comparison needs one newly prepared scoped rule".into());
        }
        let mut rebuilt = store
            .prepare_scoped_change(prepared.candidate(), &request, operation)
            .map_err(error)?;
        let mut original = prepared.clone();
        // Existing copied-input proofs were independently checked above. They
        // cannot change the population, lifecycle, operation or compiled rule.
        rebuilt.correspondences.clear();
        original.correspondences.clear();
        if rebuilt != original || prepared.operation_id() != operation {
            return Err("The prepared rule differs from the chosen population, finished-work meaning or operation".into());
        }
        check(store, basis, &cancelled)?;
        Ok(Self {
            basis: basis.clone(),
            prepared,
            request,
            operation: operation.into(),
        })
    }
    pub fn preparation(&self) -> &PreparedScopedChange {
        &self.prepared
    }
    pub fn request(&self) -> &ScopeRequest {
        &self.request
    }
    pub fn identity(&self) -> Result<Digest> {
        canonical_digest(
            IdentityDomain::Source,
            &(&self.basis, &self.prepared, &self.request, &self.operation),
        )
        .map_err(error)
    }
    fn check(&self, store: &ProductStore, current: &Self, cancelled: &AtomicBool) -> Result<()> {
        check(store, &self.basis, cancelled)?;
        if current != self {
            return Err("The selected rule or its population changed. Discover it again before using these results".into());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct RuleDiscoveryDraft {
    selection: RuleSelection,
    modify_request: DevelopmentRequest,
    modify_result: DevelopmentResult,
    request: DevelopmentRequest,
}
impl RuleDiscoveryDraft {
    #[allow(clippy::too_many_arguments)]
    pub fn after_modify(
        store: &ProductStore,
        basis: &ProjectSnapshot,
        modify_request: DevelopmentRequest,
        modify_result: DevelopmentResult,
        candidate: &str,
        selection: RuleSelection,
        id: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        selection.check(store, &selection, &cancelled)?;
        modify_request.validate().map_err(error)?;
        modify_result.validate_for(&modify_request).map_err(error)?;
        if &selection.basis != basis
            || modify_request.operation != DevelopmentOperation::Modify
            || modify_request.sources.last() != Some(basis.program().map_err(error)?)
            || modify_request.project_id != basis.data.project_id
            || modify_request.decisions != basis.decisions
            || modify_request.context.data_digest.as_ref()
                != Some(&basis.data.identity().map_err(error)?)
            || modify_request.context.session_digest.as_ref()
                != Some(&basis.session.identity().map_err(error)?)
            || id == modify_request.id
        {
            return Err("Discovery needs the exact completed Modify request and a distinct request identity".into());
        }
        let authored = modify_result
            .response
            .candidates
            .iter()
            .find(|c| c.id == candidate)
            .ok_or("The changed program is absent from the completed result")?;
        let authored = CapturedProgram::capture(
            authored.source_json.as_bytes(),
            &modify_request.project_id,
            modify_result.producer.clone(),
            None,
        )
        .map_err(error)?;
        if &authored != selection.prepared.candidate() {
            return Err("The prepared rule is not the actual returned Modify source".into());
        }
        let request = DecisionEngine::new(
            LocalRuntime::with_cancellation(cancelled.clone()),
            IntentArchive::new(store.clone()),
        )
        .inherit_request(
            basis,
            DevelopmentRequest {
                version: CONTRACT_VERSION,
                id: id.into(),
                project_id: basis.data.project_id.clone(),
                operation: DevelopmentOperation::Discover,
                request: modify_request.request.clone(),
                sources: vec![
                    basis.program().map_err(error)?.clone(),
                    selection.prepared.target().clone(),
                ],
                context: modify_request.context.clone(),
                examples: vec![],
                accepted_scenes: vec![],
                decisions: basis.decisions.clone(),
                unknowns: vec![],
                required_capabilities: modify_request.required_capabilities.clone(),
            },
        )
        .map_err(error)?;
        selection.check(store, &selection, &cancelled)?;
        Ok(Self {
            selection,
            modify_request,
            modify_result,
            request,
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
    pub fn selection(&self) -> &RuleSelection {
        &self.selection
    }
    pub fn evaluate(
        &self,
        store: &ProductStore,
        current: &RuleSelection,
        result: DevelopmentResult,
        mut policy: DiscoveryPolicy,
        lowerings: Vec<PreparedDiscoveryCandidate>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<RuleDiscoveryQueue> {
        self.selection.check(store, current, &cancelled)?;
        result.validate_for(&self.request).map_err(error)?;
        let mut history = VerifiedRetainedHistory::load(store).map_err(error)?;
        // This is the primary Modify producer. It must never acquire a forged
        // Discover lowering receipt just because the response echoes its bytes.
        history
            .map_prepared_target(self.selection.prepared.clone(), vec![])
            .map_err(error)?;
        let has_other_preparations = !lowerings.is_empty();
        for lowering in lowerings {
            history.map_prepared_result(lowering).map_err(error)?;
        }
        policy.retained_history = Some(history.clone());
        let report = discover(&self.request, &result, &policy, cancelled.clone()).map_err(error)?;
        self.selection.check(store, current, &cancelled)?;
        Ok(RuleDiscoveryQueue {
            selection: self.selection.clone(),
            report,
            history,
            has_other_preparations,
        })
    }
}

#[derive(Clone)]
pub(super) struct RuleDiscoveryQueue {
    selection: RuleSelection,
    report: DiscoveryReport,
    history: VerifiedRetainedHistory,
    has_other_preparations: bool,
}
impl RuleDiscoveryQueue {
    pub fn report(&self) -> &DiscoveryReport {
        &self.report
    }
    fn supported(&self, witness: &VerifiedWitness) -> bool {
        let current = self.selection.basis.program().expect("checked snapshot");
        witness.matches_sources(current, self.selection.prepared.target())
            || witness.matches_sources(self.selection.prepared.target(), current)
    }
    fn witness(&self, question: &str, index: usize) -> Result<&VerifiedWitness> {
        let witness = self
            .report
            .questions
            .iter()
            .find(|q| q.id == question)
            .and_then(|q| q.witnesses.get(index))
            .ok_or("This rule example is no longer available")?;
        if !self.supported(witness) {
            return Err("Only Current versus this exact prepared rule can be played here. Two new rule layers need a separate supported comparison".into());
        }
        Ok(witness)
    }
    pub fn view(&self) -> QueueView {
        let mut unverified = self.report.unverified.clone();
        if self.has_other_preparations {
            unverified.push("Only Current versus the chosen prepared rule is actionable here; another prepared rule is not a substitute for it".into());
        }
        QueueView {
            state: if !self.report.defects.is_empty() {
                QueueState::NeedsRepair
            } else if !self.report.questions.is_empty() {
                QueueState::Questions
            } else if !unverified.is_empty() {
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
                    unknowns: q.unknowns.clone(),
                    witnesses: q
                        .witnesses
                        .iter()
                        .map(|w| ExampleView {
                            labels: labels(&self.selection, w),
                            playable: false,
                            copyable: false,
                            example_issue: (!self.supported(w)).then(|| {
                                "This example is not Current versus the chosen prepared rule".into()
                            }),
                            copy_issue: None,
                        })
                        .collect(),
                })
                .collect(),
            defects: self.report.defects.clone(),
            unverified,
            coverage: self.report.coverage.clone(),
        }
    }
    pub fn checked_view(
        &self,
        store: &ProductStore,
        selection: &RuleSelection,
        cancelled: Arc<AtomicBool>,
    ) -> Result<QueueView> {
        self.selection.check(store, selection, &cancelled)?;
        let mut view = self.view();
        for question in &mut view.questions {
            for (index, witness) in question.witnesses.iter_mut().enumerate() {
                match self.open_example(store, selection, &question.id, index, cancelled.clone()) {
                    Ok(_) => witness.playable = true,
                    Err(e) => witness.example_issue = Some(e),
                }
                match self.copy_trace(store, selection, &question.id, index, cancelled.clone()) {
                    Ok(_) => witness.copyable = true,
                    Err(e) => witness.copy_issue = Some(e),
                }
            }
        }
        self.selection.check(store, selection, &cancelled)?;
        Ok(view)
    }
    pub fn open_example(
        &self,
        store: &ProductStore,
        selection: &RuleSelection,
        question: &str,
        index: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<RuleExample> {
        self.selection.check(store, selection, &cancelled)?;
        let witness = self.witness(question, index)?;
        let checked = self
            .history
            .checked_witness_replay(&self.selection.prepared, witness, cancelled.clone())
            .map_err(error)?;
        let scenario = checked.witness().witness().scenario.clone();
        let playback = run_pair(
            store,
            &self.selection,
            witness,
            &scenario,
            checked.admission(),
            cancelled.clone(),
        )?;
        self.selection.check(store, selection, &cancelled)?;
        Ok(RuleExample {
            selection: self.selection.clone(),
            scenario,
            minimal: witness
                .witness()
                .minimization
                .as_ref()
                .is_some_and(|m| m.complete),
            checked,
            playback: Some(playback),
        })
    }
    /// This separate trace uses today's frozen saved copies and gets fresh
    /// evidence. It makes no minimality claim and never invents a mapping for
    /// synthetic business references that those copies cannot execute.
    pub fn copy_trace(
        &self,
        store: &ProductStore,
        selection: &RuleSelection,
        question: &str,
        index: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<RuleCopyTrace> {
        self.selection.check(store, selection, &cancelled)?;
        let witness = self.witness(question, index)?;
        let checked = self
            .history
            .checked_witness_replay(&self.selection.prepared, witness, cancelled.clone())
            .map_err(error)?;
        let minimum = &checked.witness().witness().scenario;
        let source = self.selection.basis.program().map_err(error)?;
        let mut original =
            if operations(source, minimum).is_superset(&self.selection.request.operations) {
                minimum.clone()
            } else {
                checked.witness().initial_scenario().clone()
            };
        if operations(source, &original).is_empty() {
            return Err("This example only observes a result. Try an actual business task before recording a scoped choice".into());
        }
        original.seed = self.selection.basis.data.clone();
        original.session = self.selection.basis.session.clone();
        original.clock_day = self.selection.basis.clock_day;
        original.id = format!(
            "rule-copy-{}",
            &original.identity().map_err(error)?.as_str()[..40]
        );
        finish_observation(&mut original)?;
        let mut prepared = self.selection.prepared.clone();
        prepared.correspondences.clear();
        let context =
            ScopedExecutionContext::prepared(&self.selection.basis, &prepared).map_err(error)?;
        let mut mapped = original.clone();
        mapped.seed = crate::product_runtime::merged_data(prepared.target(), &original.seed)
            .map_err(error)?;
        let (context, scenario) = context
            .project_scenario(source, &original, prepared.target(), &mapped)
            .map_err(error)?;
        prepared.correspondences = context.correspondence_proofs();
        let playback = run_pair(store, &self.selection, witness, &scenario, Arc::new(context), cancelled.clone())
            .map_err(|e| format!("This example could not be replayed on saved copies without changing its business references: {e}"))?;
        self.selection.check(store, selection, &cancelled)?;
        Ok(RuleCopyTrace {
            selection: self.selection.clone(),
            original,
            scenario,
            prepared,
            playback,
        })
    }
}

fn labels(selection: &RuleSelection, witness: &VerifiedWitness) -> [String; 2] {
    [witness.before_program(), witness.after_program()].map(|source| {
        if selection.basis.program().ok() == Some(source) {
            "Current"
        } else if source == selection.prepared.target() {
            "Alternative"
        } else {
            "Other design"
        }
        .into()
    })
}
struct Playback {
    views: [RuntimeView; 2],
    evidence: [RunEvidence; 2],
    day: i32,
}
impl Clone for Playback {
    fn clone(&self) -> Self {
        Self {
            views: self.views.clone(),
            evidence: self.evidence.clone(),
            day: self.day,
        }
    }
}
fn run_pair(
    store: &ProductStore,
    selection: &RuleSelection,
    witness: &VerifiedWitness,
    scenario: &ScenarioSpec,
    admission: Arc<dyn ReplayAdmission>,
    cancelled: Arc<AtomicBool>,
) -> Result<Playback> {
    check(store, &selection.basis, &cancelled)?;
    let runtime =
        LocalRuntime::with_cancellation(cancelled.clone()).with_admission(admission.clone());
    let mut views = vec![];
    let mut evidence = vec![];
    let mut days = vec![];
    let mut namespaces = vec![];
    for source in [witness.before_program(), witness.after_program()] {
        scenario.validate(&source.program).map_err(error)?;
        let run = runtime
            .replay_admitted(
                source,
                scenario,
                &selection.basis.decisions,
                RuntimeLimits::default(),
                "rule-playback",
                Some(admission.as_ref()),
            )
            .map_err(error)?;
        if run.state != EvidenceState::Observed {
            return Err(format!(
                "The copied business task did not complete: {:?}",
                run.errors
            ));
        }
        evidence.push(run);
        let ids = admission
            .replay_operation_ids(source, scenario)
            .map_err(error)?;
        namespaces.push(ids.clone());
        let mut run = runtime
            .start(
                source,
                &scenario.seed,
                &scenario.session,
                scenario.clock_day,
                scenario.random_seed,
                RuntimeLimits::default(),
            )
            .map_err(error)?;
        for (input, id) in scenario.inputs.iter().zip(ids) {
            runtime.apply(&mut run, input, &id).map_err(error)?;
            admission
                .validate_state(source, runtime.data(&run), run.clock_day())
                .map_err(error)?;
        }
        views.push(runtime.view_model(&run).map_err(error)?);
        days.push(run.clock_day());
    }
    if namespaces[0] != namespaces[1] || days[0] != days[1] {
        return Err("The compared rule runs did not share one exact input sequence".into());
    }
    check(store, &selection.basis, &cancelled)?;
    Ok(Playback {
        views: [views.remove(0), views.remove(0)],
        evidence: [evidence.remove(0), evidence.remove(0)],
        day: days[0],
    })
}
fn finish_observation(scenario: &mut ScenarioSpec) -> Result<()> {
    if !matches!(scenario.inputs.last(), Some(SemanticInput::Observe { .. })) {
        let used: BTreeSet<_> = scenario
            .inputs
            .iter()
            .filter_map(|i| {
                if let SemanticInput::Observe { point } = i {
                    Some(point.as_str())
                } else {
                    None
                }
            })
            .collect();
        let point = (0..=MAX_ITEMS)
            .map(|i| format!("rule-final-{i}"))
            .find(|p| !used.contains(p.as_str()))
            .ok_or("Observation points exceed the supported limit")?;
        scenario.inputs.push(SemanticInput::Observe { point });
    }
    Ok(())
}
fn operations(source: &CapturedProgram, scene: &ScenarioSpec) -> BTreeSet<Id> {
    scene
        .inputs
        .iter()
        .filter_map(|input| match input {
            SemanticInput::Invoke { action, .. } => Some(action.clone()),
            SemanticInput::Submit { view, .. } => source
                .program
                .views
                .iter()
                .find(|v| &v.id == view)
                .and_then(|v| {
                    if let ViewKind::Form { action, .. } = &v.kind {
                        Some(action.clone())
                    } else {
                        None
                    }
                }),
            SemanticInput::Activate { view, binding, .. } => source
                .program
                .views
                .iter()
                .find(|v| &v.id == view)
                .and_then(|v| v.actions.iter().find(|a| &a.id == binding))
                .map(|a| a.action.clone()),
            SemanticInput::Control { view, control, .. } => source
                .program
                .views
                .iter()
                .find(|v| &v.id == view)
                .and_then(|v| {
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
                }),
            _ => None,
        })
        .collect()
}

#[derive(Clone)]
pub(super) struct RuleExample {
    selection: RuleSelection,
    checked: CheckedWitnessReplay,
    scenario: ScenarioSpec,
    minimal: bool,
    playback: Option<Playback>,
}
impl RuleExample {
    pub fn scenario(&self) -> &ScenarioSpec {
        &self.scenario
    }
    pub fn playback(&self) -> Result<ExamplePlayback> {
        let p = self
            .playback
            .as_ref()
            .ok_or("This example needs a fresh successful replay")?;
        Ok(ExamplePlayback {
            labels: labels(&self.selection, self.checked.witness()),
            runs: p.views.clone(),
            minimal: self.minimal,
            trial_day: p.day,
        })
    }
    pub fn evidence(&self) -> Result<&[RunEvidence; 2]> {
        Ok(&self
            .playback
            .as_ref()
            .ok_or("This example has no current replay evidence")?
            .evidence)
    }
    pub fn trial(
        &mut self,
        store: &ProductStore,
        selection: &RuleSelection,
        input: SemanticInput,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        // Clear old display/evidence authority before any failing edit or
        // cancellation; adding an input never retains the minimum certificate.
        self.minimal = false;
        self.playback = None;
        self.selection.check(store, selection, &cancelled)?;
        self.checked
            .check(store, selection.preparation(), &cancelled)
            .map_err(error)?;
        let mut scenario = self.scenario.clone();
        scenario.inputs.push(input);
        finish_observation(&mut scenario)?;
        let playback = run_pair(
            store,
            &self.selection,
            self.checked.witness(),
            &scenario,
            self.checked.admission(),
            cancelled,
        )?;
        self.scenario = scenario;
        self.playback = Some(playback);
        Ok(())
    }
}

/// Immutable checked trace for the integration owner's ChangeDraft entrypoint.
/// It exposes no AcceptedScene setter and cannot issue a choice or adoption.
#[derive(Clone)]
pub(super) struct RuleCopyTrace {
    selection: RuleSelection,
    original: ScenarioSpec,
    scenario: ScenarioSpec,
    prepared: PreparedScopedChange,
    playback: Playback,
}
impl RuleCopyTrace {
    pub fn check(
        &self,
        store: &ProductStore,
        selection: &RuleSelection,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        self.selection.check(store, selection, &cancelled)
    }
    pub fn original_scenario(&self) -> &ScenarioSpec {
        &self.original
    }
    pub fn scenario(&self) -> &ScenarioSpec {
        &self.scenario
    }
    pub fn preparation(&self) -> &PreparedScopedChange {
        &self.prepared
    }
    pub fn evidence(&self) -> &[RunEvidence; 2] {
        &self.playback.evidence
    }
    pub fn views(&self) -> &[RuntimeView; 2] {
        &self.playback.views
    }
}
