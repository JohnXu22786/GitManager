//! Private intention-history, reconciliation and withdrawal workflow pieces.
//! The shared studio owns consent, the journal, the queue and final adoption.
use super::change_adapter;
use crate::product_contract::*;
use crate::product_decisions::{
    AcceptedScene, DecisionEngine, EvolutionDraft, IntentArchive, IntentionBinding,
    ReconciliationRequest, VerifiedChange,
};
use crate::product_protocol::RuntimeView;
use crate::product_runtime::{LocalRuntime, ReplayAdmission};
use crate::product_store::{
    scope::{PreparedScopedChange, ScopePopulation, ScopedExecutionContext},
    AdoptionReceipt, ProductStore, ProjectSnapshot,
};
use crate::ui::product_runtime_view::{value_text, WidgetTrace};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn cancelled(flag: &AtomicBool) -> Result<()> {
    if flag.load(Ordering::Acquire) {
        Err("Cancelled; saved work and intentions were kept".into())
    } else {
        Ok(())
    }
}
fn fresh(store: &ProductStore, basis: &ProjectSnapshot, flag: &AtomicBool) -> Result<()> {
    cancelled(flag)?;
    if store.load().map_err(error)? != *basis {
        return Err("Saved work changed. Open a fresh preview before continuing".into());
    }
    cancelled(flag)
}

/// Shared bounded helper for discovery and reconciliation. The existing adapter
/// owns destination mapping; this helper does not compile or invent slot claims.
pub(super) fn prepare_managed_design(
    store: &ProductStore,
    basis: &ProjectSnapshot,
    candidate: &CapturedProgram,
    operation: &str,
    cancel: Arc<AtomicBool>,
) -> Result<PreparedScopedChange> {
    fresh(store, basis, &cancel)?;
    let mappings = change_adapter::slot_mappings(basis, candidate)?;
    let prepared = store
        .prepare_managed_evolution(candidate, &mappings, operation)
        .map_err(error)?;
    ScopedExecutionContext::prepared(basis, &prepared).map_err(error)?;
    fresh(store, basis, &cancel)?;
    Ok(prepared)
}

#[derive(Clone)]
pub(super) struct HistoricalScene {
    pub accepted: AcceptedScene,
    /// The entire original capture, including source bytes and producer binding.
    pub source: Digest,
}
#[derive(Clone)]
pub(super) struct DecisionView {
    pub decision: ScopedDecision,
    pub status_text: String,
    pub scope_text: String,
    pub binding: IntentionBinding,
    /// Original source-qualified evidence, not a claim about today's behavior.
    pub scenes: Vec<HistoricalScene>,
    /// Exact receipt membership, not guessed creation/ownership by label.
    pub receipts: Vec<AdoptionReceipt>,
}
#[derive(Clone)]
pub(super) struct LayerView {
    pub id: Digest,
    pub active: bool,
    pub operation: Id,
    pub scope_text: String,
    pub decisions: Vec<Id>,
    pub receipt: AdoptionReceipt,
}
#[derive(Clone)]
pub(super) struct HistoryView {
    pub decisions: Vec<DecisionView>,
    pub layers: Vec<LayerView>,
    pub receipts: Vec<AdoptionReceipt>,
}
fn reconcilable(decision: &ScopedDecision) -> bool {
    decision.status == DecisionStatus::Active
        || (decision.status == DecisionStatus::Pending
            && matches!(
                decision.outcome,
                DecisionOutcome::BothNeeded | DecisionOutcome::EitherAcceptable
            ))
}
fn outcome_text(decision: &ScopedDecision, binding: IntentionBinding) -> &'static str {
    match decision.outcome {
        DecisionOutcome::Accept { .. } => match binding {
            IntentionBinding::ObservedOutcome => "Chose this design; recorded promise: keep the experienced result",
            IntentionBinding::PropertiesOnly => "Chose this design; recorded promise: keep the explicitly chosen conditions",
        },
        DecisionOutcome::KeepCurrent => match binding {
            IntentionBinding::ObservedOutcome => "Kept the then-current behavior; recorded promise: keep the experienced result",
            IntentionBinding::PropertiesOnly => "Kept the then-current behavior; recorded promise: keep the explicitly chosen conditions",
        },
        DecisionOutcome::EitherAcceptable => {
            "Either option is acceptable; no single option was chosen"
        }
        DecisionOutcome::BothNeeded => {
            "Both ways of working are needed; this choice has not saved a combined design"
        }
        DecisionOutcome::NeitherFits => "Neither example fits; neither result was accepted",
        DecisionOutcome::Deferred => "Decision deferred; no result was accepted",
    }
}
fn scope_text(scope: &DecisionScope) -> String {
    let population = match &scope.population {
        Population::All => "All work".into(),
        Population::NewWork => "New work".into(),
        Population::CreatedAfter { generation } => {
            format!("Work created after saved data generation {generation}")
        }
        Population::Records { records } => format!("{} selected work records", records.len()),
        Population::Entity { entity } => format!("Work in {entity}"),
        Population::Where { entity, .. } => {
            format!("Work in {entity} matching the saved business condition")
        }
    };
    format!("{population}; actions: {}; {} excluded records; {} saved conditions; {} unresolved boundaries",
        scope.operations.iter().cloned().collect::<Vec<_>>().join(", "), scope.excluded_records.len(), scope.conditions.len(), scope.unknowns.len())
}
impl HistoryView {
    pub fn load(
        store: &ProductStore,
        basis: &ProjectSnapshot,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        fresh(store, basis, &cancel)?;
        let archive = IntentArchive::new(store.clone());
        let engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(cancel.clone()),
            archive.clone(),
        );
        let mut decisions = vec![];
        for decision in &basis.decisions.decisions {
            cancelled(&cancel)?;
            let scenes = archive
                .accepted_scenes(decision)
                .map_err(error)?
                .into_iter()
                .map(|accepted| {
                    Ok(HistoricalScene {
                        source: canonical_digest(IdentityDomain::Source, accepted.program())
                            .map_err(error)?,
                        accepted,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let status_text = match &decision.status {
                DecisionStatus::Active => "In use".into(),
                DecisionStatus::Pending => "Still to decide".into(),
                DecisionStatus::Superseded { by } => format!("Replaced by {by}"),
                DecisionStatus::Withdrawn { adoption } => {
                    format!("Withdrawn in saved change {adoption}")
                }
            };
            let receipts = basis
                .adoptions
                .iter()
                .filter(|receipt| {
                    receipt.plan.required_decisions.contains(&decision.id)
                        || receipt.plan.retire_decisions.contains(&decision.id)
                        || basis.scope.adoptions.iter().any(|scope| {
                            scope.revision == receipt.revision
                                && scope.decisions.contains(&decision.id)
                        })
                })
                .cloned()
                .collect();
            decisions.push(DecisionView {
                decision: decision.clone(),
                status_text,
                scope_text: scope_text(&decision.scope),
                binding: engine.intention_binding(decision).map_err(error)?,
                scenes,
                receipts,
            });
        }
        let active = basis
            .scope
            .compositions
            .get(&basis.active_revision)
            .map(|m| &m.active);
        let mut layers = vec![];
        for (id, layer) in &basis.scope.layers {
            let receipt = basis
                .adoptions
                .iter()
                .find(|r| r.plan.id == layer.operation)
                .ok_or("The behavior layer has no saved adoption receipt")?;
            let scoped = basis
                .scope
                .adoptions
                .iter()
                .find(|r| r.revision == receipt.revision)
                .ok_or("The behavior layer has no saved intention receipt")?;
            layers.push(LayerView {
                id: id.clone(),
                active: active.is_some_and(|set| set.contains(id)),
                operation: layer.operation.clone(),
                scope_text: match &layer.request.population {
                    ScopePopulation::All => "All work".into(),
                    ScopePopulation::FutureWork => {
                        "Only work created after this rule was saved".into()
                    }
                    ScopePopulation::SelectedUnfinished { records } => {
                        format!("{} selected unfinished records", records.len())
                    }
                },
                decisions: scoped.decisions.clone(),
                receipt: receipt.clone(),
            });
        }
        fresh(store, basis, &cancel)?;
        Ok(Self {
            decisions,
            layers,
            receipts: basis.adoptions.clone(),
        })
    }
}

/// Ordinary reconciliation must receive the actual already-authorized provider.
/// Its `develop` is invoked exactly once, by the decision engine. The caller
/// retains the provider's raw response and receipt even if verification fails.
pub(super) struct Reconciliation {
    basis: ProjectSnapshot,
    request: ReconciliationRequest,
    evolution: Id,
    operation: Id,
}
impl Reconciliation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: &ProductStore,
        basis: &ProjectSnapshot,
        engine: &DecisionEngine<LocalRuntime>,
        request_id: &str,
        text: &str,
        needs: &[Id],
        evolution: &str,
        operation: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        fresh(store, basis, &cancel)?;
        if !valid_id(evolution) || !valid_id(operation) {
            return Err("Invalid new-design operation identity".into());
        }
        if needs.iter().any(|id| {
            basis
                .decisions
                .decisions
                .iter()
                .find(|d| &d.id == id)
                .is_some_and(|d| !reconcilable(d))
        }) {
            return Err("This choice has no accepted result to preserve. Make a fresh outcome decision first".into());
        }
        let instruction = format!("{text}\nReturn the executable evolution using this exact evolution suggestion ID: {evolution}");
        let request = engine
            .reconciliation_request(basis, request_id, &instruction, needs)
            .map_err(error)?;
        fresh(store, basis, &cancel)?;
        Ok(Self {
            basis: basis.clone(),
            request,
            evolution: evolution.into(),
            operation: operation.into(),
        })
    }
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
    pub fn develop<P: DevelopmentProvider>(
        &self,
        store: &ProductStore,
        engine: &DecisionEngine<LocalRuntime>,
        provider: &P,
        cancel: Arc<AtomicBool>,
    ) -> Result<Design> {
        fresh(store, &self.basis, &cancel)?;
        if self
            .basis
            .scope
            .compositions
            .contains_key(&self.basis.active_revision)
        {
            return Err("This saved tool needs the managed result preparation route".into());
        }
        let draft = engine
            .develop_evolution(provider, &self.request, &self.evolution, &|| {
                cancel.load(Ordering::Acquire)
            })
            .map_err(error)?;
        fresh(store, &self.basis, &cancel)?;
        Design::new(store, self, engine, draft, None, cancel)
    }
    /// A genuine result from the shared authorized request, never a fabricated
    /// provider wrapper or a second invocation. The common worker owns transport
    /// correlation and exact raw-result/receipt validation before this dispatch.
    pub fn develop_prepared(
        &self,
        store: &ProductStore,
        engine: &DecisionEngine<LocalRuntime>,
        result: DevelopmentResult,
        cancel: Arc<AtomicBool>,
    ) -> Result<Design> {
        fresh(store, &self.basis, &cancel)?;
        result.response.validate_for(&self.request).map_err(error)?;
        let suggestion = result
            .response
            .evolutions
            .iter()
            .find(|e| e.id == self.evolution)
            .ok_or("The provider returned no requested executable new design")?;
        let candidate = result
            .response
            .candidates
            .iter()
            .find(|c| c.id == suggestion.candidate)
            .ok_or("The new design's executable is missing")?;
        let candidate = CapturedProgram::capture(
            candidate.source_json.as_bytes(),
            &self.basis.data.project_id,
            result.producer.clone(),
            None,
        )
        .map_err(error)?;
        let prepared = prepare_managed_design(
            store,
            &self.basis,
            &candidate,
            &self.operation,
            cancel.clone(),
        )?;
        let draft = engine
            .develop_prepared_evolution(
                result,
                &self.request,
                &self.evolution,
                prepared.clone(),
                &|| cancel.load(Ordering::Acquire),
            )
            .map_err(error)?;
        fresh(store, &self.basis, &cancel)?;
        Design::new(store, self, engine, draft, Some(prepared), cancel)
    }
}

#[derive(Clone)]
pub(super) struct DesignView {
    pub operation: Id,
    pub authored: CapturedProgram,
    pub candidate: CapturedProgram,
    pub retire: Vec<Id>,
    pub preserve: Vec<Id>,
    pub preview: RuntimeView,
}
pub(super) struct Design {
    basis: ProjectSnapshot,
    operation: Id,
    draft: EvolutionDraft,
    preview: CopyPreview,
    view: DesignView,
}
impl Design {
    fn new(
        store: &ProductStore,
        request: &Reconciliation,
        engine: &DecisionEngine<LocalRuntime>,
        draft: EvolutionDraft,
        prepared: Option<PreparedScopedChange>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        // Availability comes from the actual adoption gate, not just a response.
        let checked = engine
            .prepare_evolution(store, &draft, &request.operation)
            .map_err(error)?;
        if checked.candidate() != draft.candidate() {
            return Err("New-design target differs from its verified adoption".into());
        }
        let preview = CopyPreview::new(
            &request.basis,
            draft.candidate(),
            prepared,
            &request.operation,
            cancel.clone(),
        )?;
        let view = DesignView {
            operation: request.operation.clone(),
            authored: draft.authored_candidate().clone(),
            candidate: draft.candidate().clone(),
            retire: checked.plan().retire_decisions.clone(),
            preserve: request
                .basis
                .decisions
                .decisions
                .iter()
                .filter(|d| {
                    matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                        && !checked.plan().retire_decisions.contains(&d.id)
                })
                .map(|d| d.id.clone())
                .collect(),
            preview: preview.view.clone(),
        };
        fresh(store, &request.basis, &cancel)?;
        Ok(Self {
            basis: request.basis.clone(),
            operation: request.operation.clone(),
            draft,
            preview,
            view,
        })
    }
    pub fn view(&self) -> DesignView {
        self.view.clone()
    }
    pub fn trial(
        &mut self,
        store: &ProductStore,
        input: SemanticInput,
        cancel: Arc<AtomicBool>,
    ) -> Result<RuntimeView> {
        fresh(store, &self.basis, &cancel)?;
        let (scenario, view) = self.preview.trial(&self.basis, input, cancel.clone())?;
        fresh(store, &self.basis, &cancel)?;
        self.preview.scenario = scenario;
        self.preview.view = view.clone();
        self.view.preview = view.clone();
        Ok(view)
    }
    pub fn decision(
        &self,
        store: &ProductStore,
        engine: &DecisionEngine<LocalRuntime>,
        cancel: Arc<AtomicBool>,
    ) -> Result<VerifiedChange> {
        fresh(store, &self.basis, &cancel)?;
        let checked = engine
            .prepare_evolution(store, &self.draft, &self.operation)
            .map_err(error)?;
        if checked.candidate() != &self.view.candidate
            || checked.plan().retire_decisions != self.view.retire
        {
            return Err("New-design adoption differs from the previewed design".into());
        }
        fresh(store, &self.basis, &cancel)?;
        Ok(checked)
    }
}

#[derive(Clone)]
pub(super) struct WithdrawalView {
    pub operation: Id,
    pub layers: Vec<Digest>,
    pub retire: Vec<Id>,
    pub preserve: Vec<Id>,
    pub candidate: CapturedProgram,
    pub preview: RuntimeView,
}
pub(super) struct Withdrawal {
    basis: ProjectSnapshot,
    prepared: PreparedScopedChange,
    checked: VerifiedChange,
    preview: CopyPreview,
    view: WithdrawalView,
}
impl Withdrawal {
    pub fn prepare(
        store: &ProductStore,
        basis: &ProjectSnapshot,
        engine: &DecisionEngine<LocalRuntime>,
        layers: &[Digest],
        operation: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        fresh(store, basis, &cancel)?;
        // Keep this independently regenerated preparation: VerifiedChange is
        // intentionally opaque. Both calls bind the same store, layers and ID.
        let prepared = store
            .prepare_scoped_withdrawal(layers, operation)
            .map_err(error)?;
        let checked = engine
            .prepare_scoped_withdrawal(store, layers, operation)
            .map_err(error)?;
        if prepared.operation_id() != checked.plan().id
            || prepared.target() != checked.candidate()
            || checked.plan().expected_data != basis.data.identity().map_err(error)?
        {
            return Err("Withdrawal preview and verified adoption differ".into());
        }
        let preview = CopyPreview::new(
            basis,
            checked.candidate(),
            Some(prepared.clone()),
            operation,
            cancel.clone(),
        )?;
        let view = WithdrawalView {
            operation: operation.into(),
            layers: layers.to_vec(),
            retire: checked.plan().retire_decisions.clone(),
            preserve: basis
                .decisions
                .decisions
                .iter()
                .filter(|d| {
                    matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                        && !checked.plan().retire_decisions.contains(&d.id)
                })
                .map(|d| d.id.clone())
                .collect(),
            candidate: checked.candidate().clone(),
            preview: preview.view.clone(),
        };
        fresh(store, basis, &cancel)?;
        Ok(Self {
            basis: basis.clone(),
            prepared,
            checked,
            preview,
            view,
        })
    }
    pub fn view(&self) -> WithdrawalView {
        self.view.clone()
    }
    pub fn trial(
        &mut self,
        store: &ProductStore,
        input: SemanticInput,
        cancel: Arc<AtomicBool>,
    ) -> Result<RuntimeView> {
        fresh(store, &self.basis, &cancel)?;
        let (scenario, view) = self.preview.trial(&self.basis, input, cancel.clone())?;
        fresh(store, &self.basis, &cancel)?;
        self.preview.scenario = scenario;
        self.preview.view = view.clone();
        self.view.preview = view.clone();
        Ok(view)
    }
    pub fn decision(
        &self,
        store: &ProductStore,
        engine: &DecisionEngine<LocalRuntime>,
        cancel: Arc<AtomicBool>,
    ) -> Result<VerifiedChange> {
        fresh(store, &self.basis, &cancel)?;
        ScopedExecutionContext::prepared(&self.basis, &self.prepared).map_err(error)?;
        let checked = engine
            .prepare_scoped_withdrawal(store, &self.view.layers, &self.view.operation)
            .map_err(error)?;
        if checked.candidate() != self.prepared.target() || checked.plan() != self.checked.plan() {
            return Err("Withdrawal changed since the checked preview".into());
        }
        fresh(store, &self.basis, &cancel)?;
        Ok(checked)
    }
}

/// A copied target-only runtime. No replay is a saved business operation, and
/// copied artifacts never enter the live export inventory. Inputs and their
/// deterministic operation namespace are replayed from the same checked seed.
struct CopyPreview {
    candidate: CapturedProgram,
    prepared: Option<PreparedScopedChange>,
    scenario: ScenarioSpec,
    view: RuntimeView,
}
impl CopyPreview {
    fn new(
        basis: &ProjectSnapshot,
        candidate: &CapturedProgram,
        prepared: Option<PreparedScopedChange>,
        operation: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        let seed = match &prepared {
            Some(p) if p.target() == candidate && p.operation_id() == operation => p.seed().clone(),
            Some(_) => {
                return Err("Copied work belongs to another prepared target or operation".into())
            }
            None => crate::product_runtime::merged_data(candidate, &basis.data).map_err(error)?,
        };
        let scenario = ScenarioSpec {
            version: CONTRACT_VERSION,
            id: operation.into(),
            label: "Try the change on copied current work".into(),
            seed,
            // Adoption opens the target initial session; do not require an old
            // view/state shape to survive a genuinely new executable design.
            session: SessionState::initial(&candidate.program).map_err(error)?,
            clock_day: basis.clock_day,
            random_seed: 0,
            inputs: vec![],
            validity: vec![],
        };
        let view = Self::replay(basis, candidate, prepared.as_ref(), &scenario, cancel)?;
        Ok(Self {
            candidate: candidate.clone(),
            prepared,
            scenario,
            view,
        })
    }
    fn trial(
        &self,
        basis: &ProjectSnapshot,
        input: SemanticInput,
        cancel: Arc<AtomicBool>,
    ) -> Result<(ScenarioSpec, RuntimeView)> {
        let mut scenario = self.scenario.clone();
        scenario.inputs.push(input);
        let view = Self::replay(
            basis,
            &self.candidate,
            self.prepared.as_ref(),
            &scenario,
            cancel,
        )?;
        Ok((scenario, view))
    }
    fn replay(
        basis: &ProjectSnapshot,
        candidate: &CapturedProgram,
        prepared: Option<&PreparedScopedChange>,
        scenario: &ScenarioSpec,
        cancel: Arc<AtomicBool>,
    ) -> Result<RuntimeView> {
        cancelled(&cancel)?;
        scenario.validate(&candidate.program).map_err(error)?;
        // `prepared`, unlike `rehearsed`, explicitly regenerates Withdrawal.
        let context = prepared
            .map(|p| ScopedExecutionContext::prepared(basis, p))
            .transpose()
            .map_err(error)?;
        if let Some(context) = &context {
            context
                .validate_seed(candidate, &scenario.seed, scenario.clock_day)
                .map_err(error)?;
        }
        let mut runtime = LocalRuntime::with_cancellation(cancel.clone());
        if let Some(context) = &context {
            runtime = runtime.with_admission(Arc::new(context.clone()));
        }
        let mut run = runtime
            .start(
                candidate,
                &scenario.seed,
                &scenario.session,
                scenario.clock_day,
                scenario.random_seed,
                RuntimeLimits::default(),
            )
            .map_err(error)?;
        let operations = match &context {
            Some(c) => c.replay_operation_ids(candidate, scenario).map_err(error)?,
            None => scenario.replay_operation_ids().map_err(error)?,
        };
        for (input, operation) in scenario.inputs.iter().zip(operations) {
            runtime.apply(&mut run, input, &operation).map_err(error)?;
            if let Some(context) = &context {
                context
                    .validate_state(candidate, runtime.data(&run), run.clock_day())
                    .map_err(error)?;
            }
        }
        let view = runtime.view_model(&run).map_err(error)?;
        cancelled(&cancel)?;
        Ok(view)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Reconcile { needs: Vec<Id>, request: String },
    PreviewWithdrawal { layers: Vec<Digest> },
    AcceptDesign { operation: Id },
    Withdraw { operation: Id },
    ReturnToWork,
}
#[derive(Default)]
pub(super) struct HistoryState {
    needs: BTreeSet<Id>,
    layers: BTreeSet<Digest>,
    request: String,
}
impl HistoryState {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: &HistoryView,
        interactive: bool,
    ) -> (Option<Event>, WidgetTrace) {
        let mut trace = WidgetTrace::default();
        let mut event = None;
        self.needs.retain(|id| {
            view.decisions
                .iter()
                .any(|d| &d.decision.id == id && reconcilable(&d.decision))
        });
        self.layers
            .retain(|id| view.layers.iter().any(|l| &l.id == id && l.active));
        trace.label(ui, "Your saved ways of working");
        trace.label(ui, "These are the original compared examples, recorded choices and saved receipts. They are history, not a fresh test of today's work.");
        for item in &view.decisions {
            ui.push_id(&item.decision.id, |ui| {
                let eligible = reconcilable(&item.decision);
                let mut selected = self.needs.contains(&item.decision.id);
                let response = trace.control(
                    &format!("intention-need-{}", item.decision.id),
                    ui.add_enabled(
                        interactive && eligible,
                        egui::Checkbox::new(&mut selected, &item.decision.request),
                    ),
                );
                if response.changed() {
                    if selected {
                        self.needs.insert(item.decision.id.clone());
                    } else {
                        self.needs.remove(&item.decision.id);
                    }
                }
                trace.label(ui, format!("{} · {}", item.status_text, item.scope_text));
                trace.label(ui, format!("{}; {} recorded examples", outcome_text(&item.decision, item.binding), item.scenes.len()));
                let detail = ui.collapsing("Examples and saved change receipts", |ui| {
                    trace.label(ui, format!("Intention {}", item.decision.id));
                    for scene in &item.scenes { scene.show(ui, &mut trace); }
                    for property in &item.decision.obligations { trace.label(ui, &property.description); }
                    if item.receipts.is_empty() {
                        trace.label(ui, "No exact intention-to-receipt membership is recorded. The complete saved-change inventory is below.");
                    }
                    for receipt in &item.receipts { trace.label(ui, format!("Checked or retired in saved change {} · revision {}", receipt.plan.id, receipt.revision)); }
                });
                trace.control(&format!("intention-details-{}", item.decision.id), detail.header_response);
            });
        }
        let response = ui.add_enabled(
            interactive,
            egui::TextEdit::multiline(&mut self.request)
                .hint_text("What should a new design make possible?")
                .char_limit(MAX_TEXT_BYTES),
        );
        trace.control("intention-request", response);
        let count: usize = view
            .decisions
            .iter()
            .filter(|d| self.needs.contains(&d.decision.id))
            .map(|d| d.scenes.len())
            .sum();
        if trace.button(
            ui,
            "intention-reconcile",
            "Find a design that keeps these needs",
            interactive && !self.needs.is_empty() && count >= 2,
        ) {
            event = Some(Event::Reconcile {
                needs: self.needs.iter().cloned().collect(),
                request: if self.request.trim().is_empty() {
                    "Support these accepted ways of working together".into()
                } else {
                    self.request.clone()
                },
            });
        }
        trace.label(ui, "Behavior rules");
        trace.label(ui, "Withdrawing a rule keeps later work records and history. A copied preview and compatibility check come first.");
        for layer in &view.layers {
            let mut selected = self.layers.contains(&layer.id);
            let response = trace.control(
                &format!("intention-layer-{}", layer.id.as_str()),
                ui.add_enabled(
                    interactive && layer.active,
                    egui::Checkbox::new(
                        &mut selected,
                        format!(
                            "{} · {} · {}",
                            layer.operation,
                            layer.scope_text,
                            if layer.active { "In use" } else { "Retired" }
                        ),
                    ),
                ),
            );
            if response.changed() {
                if selected {
                    self.layers.insert(layer.id.clone());
                } else {
                    self.layers.remove(&layer.id);
                }
            }
            ui.label(format!(
                "Saved change {} · intentions {}",
                layer.receipt.plan.id,
                layer.decisions.join(", ")
            ));
        }
        if trace.button(
            ui,
            "intention-preview-withdrawal",
            "Preview withdrawing selected rules",
            interactive && !self.layers.is_empty(),
        ) {
            event = Some(Event::PreviewWithdrawal {
                layers: self.layers.iter().cloned().collect(),
            });
        }
        trace.label(ui, "All saved changes");
        trace.label(ui, "Verified receipt inventory. No birth association is inferred for an intention without exact recorded membership.");
        for receipt in &view.receipts {
            trace.label(
                ui,
                format!(
                    "Saved change {} · revision {} · source {} → {}",
                    receipt.plan.id,
                    receipt.revision,
                    receipt.previous.as_str(),
                    receipt.active.as_str()
                ),
            );
        }
        if trace.button(ui, "intention-return", "Return to saved work", interactive) {
            event = Some(Event::ReturnToWork);
        }
        (event, trace)
    }
}
impl HistoricalScene {
    fn show(&self, ui: &mut egui::Ui, trace: &mut WidgetTrace) {
        let scene = &self.accepted;
        let program = &scene.program().program;
        trace.label(
            ui,
            format!(
                "Historical example: {} · original program {} · scene {}",
                scene.scenario().label,
                program.label,
                scene.scenario().id
            ),
        );
        trace.label(
            ui,
            format!("Original source capture {}", self.source.as_str()),
        );
        trace.label(
            ui,
            format!(
                "Original producer {:?} · source bytes {}",
                scene.program().binding.producer,
                scene.program().binding.source_digest.as_str()
            ),
        );
        for observed in scene.observations() {
            let view = program.views.iter().find(|v| v.id == observed.view.view);
            trace.label(
                ui,
                format!(
                    "Recorded result at {}: {} visible rows, {} selected",
                    observed.point,
                    observed.view.rows.len(),
                    observed.view.selected.len()
                ),
            );
            for (id, value) in &observed.values {
                let label = program
                    .observables
                    .iter()
                    .find(|o| &o.id == id)
                    .map(|o| o.label.as_str())
                    .unwrap_or(id);
                trace.label(ui, format!("{label}: {}", value_text(value)));
            }
            let columns = match view.map(|v| &v.kind) {
                Some(ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. }) => {
                    columns.as_slice()
                }
                _ => &[],
            };
            for row in observed.view.rows.iter().take(20) {
                let cells = row
                    .cells
                    .iter()
                    .map(|(id, value)| {
                        let label = columns
                            .iter()
                            .find(|c| &c.id == id)
                            .map(|c| c.label.as_str())
                            .unwrap_or(id);
                        format!("{label}: {}", value_text(value))
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                trace.label(
                    ui,
                    format!("{} / {} · {cells}", row.record.entity, row.record.record),
                );
            }
            if observed.view.rows.len() > 20 {
                trace.label(
                    ui,
                    "Showing the first 20 visible rows of this retained example",
                );
            }
            for artifact in &observed.outputs {
                let label = program
                    .outputs
                    .iter()
                    .find(|o| o.id == artifact.output)
                    .map(|o| o.label.as_str())
                    .unwrap_or(&artifact.output);
                trace.label(ui, format!("{label}: {} output rows", artifact.rows.len()));
                for row in artifact.rows.iter().take(20) {
                    trace.label(
                        ui,
                        artifact
                            .columns
                            .iter()
                            .map(|column| {
                                format!("{}: {}", column.label, value_text(&row[&column.id]))
                            })
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                }
                if artifact.rows.len() > 20 {
                    trace.label(
                        ui,
                        "Showing the first 20 output rows of this retained example",
                    );
                }
            }
        }
    }
}
impl DesignView {
    pub fn show(&self, ui: &mut egui::Ui, interactive: bool) -> (Option<Event>, WidgetTrace) {
        let mut trace = WidgetTrace::default();
        trace.label(ui, "Try the new design on copied work");
        trace.label(
            ui,
            format!(
                "Proposed replacement of: {}. Independent needs kept: {}",
                self.retire.join(", "),
                self.preserve.join(", ")
            ),
        );
        let event = if trace.button(
            ui,
            "intention-accept-design",
            "Use this checked design",
            interactive,
        ) {
            Some(Event::AcceptDesign {
                operation: self.operation.clone(),
            })
        } else if trace.button(
            ui,
            "intention-discard-design",
            "Discard copied edits and return",
            interactive,
        ) {
            Some(Event::ReturnToWork)
        } else {
            None
        };
        (event, trace)
    }
}
impl WithdrawalView {
    pub fn show(&self, ui: &mut egui::Ui, interactive: bool) -> (Option<Event>, WidgetTrace) {
        let mut trace = WidgetTrace::default();
        trace.label(ui, "Preview withdrawing behavior on copied current work");
        trace.label(
            ui,
            format!(
                "Intentions retired: {}. Other needs kept: {}",
                self.retire.join(", "),
                self.preserve.join(", ")
            ),
        );
        trace.label(ui, "Saved records, later edits, completed facts, events and earlier outputs stay. Previously performed external actions are not undone.");
        let event = if trace.button(
            ui,
            "intention-withdraw",
            "Withdraw these checked rules",
            interactive,
        ) {
            Some(Event::Withdraw {
                operation: self.operation.clone(),
            })
        } else if trace.button(
            ui,
            "intention-discard-withdrawal",
            "Discard copied edits and return",
            interactive,
        ) {
            Some(Event::ReturnToWork)
        } else {
            None
        };
        (event, trace)
    }
}
