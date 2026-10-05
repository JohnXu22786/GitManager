//! Generated-tool workspace. The host owns runtime execution, durable decisions,
//! exact source/data checks and immutable adoption plans. No display model grants
//! authority to commit; this component emits requests with their observed context.
use super::product_runtime_view::{
    take_shortcuts, text_edit_focused, value_text, ProductRuntimeView, WidgetTrace,
};
use crate::product_contract::*;
use crate::product_protocol::*;
use egui::{ScrollArea, TextEdit, Ui};

#[derive(Default)]
pub struct WorkspaceOutput {
    pub request: Option<WorkspaceRequest>,
    pub trace: WidgetTrace,
}
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    project: Id,
    generation: u64,
    program: Digest,
    data: Digest,
    session: Digest,
}
impl Identity {
    fn of(view: &WorkspaceView) -> Option<Self> {
        view.active.as_ref().map(|v| Self {
            project: v.project_id.clone(),
            generation: v.generation,
            program: v.program.clone(),
            data: v.data.clone(),
            session: v.session.clone(),
        })
    }
}
struct ScopeDraft {
    source: ScopeView,
    scope: DecisionScope,
    rationale: String,
    dirty: bool,
}
#[derive(Default)]
pub struct ProductWorkspace {
    session_id: Option<Id>,
    identity: Option<Identity>,
    epoch: u64,
    text_focused: bool,
    pending: Option<RequestContext>,
    pending_replay: bool,
    recovered_pending: bool,
    comparison_editor: Option<String>,
    comparison_keyboard_side: Option<String>,
    last_request: Option<Id>,
    delivery: Option<u64>,
    status: Option<OperationOutcome>,
    need: String,
    change: Option<String>,
    scope: Option<ScopeDraft>,
    daily: ProductRuntimeView,
    before: ProductRuntimeView,
    after: ProductRuntimeView,
    witness: Option<(Id, Option<Digest>, Option<Digest>)>,
    witness_basis: Option<Identity>,
    plan_basis: Option<(Id, Option<Identity>)>,
}
impl ProductWorkspace {
    fn reset_comparison(&mut self) {
        self.before = ProductRuntimeView::default();
        self.after = ProductRuntimeView::default();
        self.comparison_editor = None;
        self.comparison_keyboard_side = None;
    }
    fn sync(&mut self, view: &WorkspaceView) {
        let first_view = self.session_id.as_ref() != Some(&view.session_id);
        if first_view {
            *self = Self {
                session_id: Some(view.session_id.clone()),
                ..Self::default()
            };
        }
        if first_view {
            if let Some(op) = view.operation.as_ref().filter(|op| {
                let active = view.active.as_ref();
                operation_pending(&op.outcome)
                    && op.context.session_id == view.session_id
                    && op.context.project_id.as_ref() == active.map(|v| &v.project_id)
                    && op.context.generation == active.map(|v| v.generation)
                    && op.context.program.as_ref() == active.map(|v| &v.program)
                    && op.context.data.as_ref() == active.map(|v| &v.data)
                    && op.context.session.as_ref() == active.map(|v| &v.session)
            }) {
                self.pending = Some(op.context.clone());
                self.recovered_pending = true;
                self.last_request = Some(op.context.request_id.clone());
                self.epoch = op.context.input_epoch;
            }
        }
        let matching = view.operation.as_ref().filter(|op| {
            self.pending.as_ref() == Some(&op.context)
                && self.delivery.is_none_or(|last| op.delivery_id > last)
        });
        let completed = matching.is_some_and(|op| !operation_pending(&op.outcome));
        let successful = matching.is_some_and(|op| match op.outcome {
            OperationOutcome::PreviewReady => true,
            OperationOutcome::Committed { generation } => view
                .active
                .as_ref()
                .is_some_and(|active| active.generation == generation),
            _ => false,
        });
        let replay_completed = successful && self.pending_replay;
        let identity = Identity::of(view);
        if self.identity != identity {
            let source_changed = self.identity.as_ref().map(|i| (&i.project, &i.program))
                != identity.as_ref().map(|i| (&i.project, &i.program));
            // Preserve unsent text only across the acknowledged input that caused
            // this update. Unrelated refreshes must not replay an old draft.
            if self.identity.is_some() && (!successful || source_changed) {
                self.daily = ProductRuntimeView::default();
                self.reset_comparison();
                self.change = None;
                self.scope = None;
                self.status = None;
                if !completed {
                    self.pending = None;
                }
            }
            self.identity = identity;
        }
        if let Some(op) = view.operation.as_ref().filter(|op| {
            self.pending.as_ref() == Some(&op.context)
                && self.delivery.is_none_or(|last| op.delivery_id > last)
        }) {
            self.delivery = Some(op.delivery_id);
            self.status = Some(op.outcome.clone());
            if !operation_pending(&op.outcome) {
                self.daily.acknowledge(successful);
                self.before.acknowledge(successful);
                self.after.acknowledge(successful);
                self.pending = None;
            }
        }
        if self.pending.is_none() {
            self.recovered_pending = false;
        }
        let witness = view.comparison.as_ref().map(|c| {
            (
                c.witness_id.clone(),
                c.before.evidence.clone(),
                c.after.evidence.clone(),
            )
        });
        if self.witness != witness {
            let same_witness =
                self.witness.as_ref().map(|key| &key.0) == witness.as_ref().map(|key| &key.0);
            if !same_witness || !replay_completed {
                self.reset_comparison();
            }
            self.witness = witness;
            self.witness_basis = self.identity.clone();
            self.scope = None;
        }
        if let Some(scope) = &view.scope {
            if let Some(draft) = &mut self.scope {
                let same_choice = draft.source.choice_id == scope.choice_id
                    && draft.source.witness_id == scope.witness_id
                    && draft.source.outcome == scope.outcome;
                let edited = draft.dirty;
                if same_choice && edited {
                    // A returned rehearsal cannot overwrite newer unsent edits.
                    draft.source = scope.clone();
                } else if draft.source != *scope {
                    self.scope = None;
                }
            }
            if self.scope.is_none() {
                self.scope = Some(ScopeDraft {
                    source: scope.clone(),
                    scope: scope.proposed_scope.clone(),
                    rationale: scope.rationale.clone().unwrap_or_default(),
                    dirty: false,
                });
            }
        } else {
            self.scope = None;
        }
        match &view.adoption {
            Some(plan)
                if self
                    .plan_basis
                    .as_ref()
                    .is_none_or(|(id, _)| id != &plan.plan_id) =>
            {
                self.plan_basis = Some((plan.plan_id.clone(), self.identity.clone()));
            }
            None => self.plan_basis = None,
            _ => {}
        }
    }
    fn request(
        &mut self,
        view: &WorkspaceView,
        action: WorkspaceAction,
    ) -> Option<WorkspaceRequest> {
        if self.last_request.as_ref() == Some(&view.next_operation_id) {
            return None;
        }
        self.epoch = self.epoch.checked_add(1)?;
        let active = view.active.as_ref();
        let context = RequestContext {
            request_id: view.next_operation_id.clone(),
            session_id: view.session_id.clone(),
            project_id: active.map(|v| v.project_id.clone()),
            generation: active.map(|v| v.generation),
            program: active.map(|v| v.program.clone()),
            data: active.map(|v| v.data.clone()),
            session: active.map(|v| v.session.clone()),
            input_epoch: self.epoch,
        };
        if matches!(
            action,
            WorkspaceAction::Back | WorkspaceAction::Close | WorkspaceAction::Cancel { .. }
        ) {
            self.daily = ProductRuntimeView::default();
            self.reset_comparison();
        }
        if matches!(action, WorkspaceAction::RestartComparison { .. }) {
            self.reset_comparison();
        }
        if matches!(action, WorkspaceAction::SetScope { .. }) {
            if let Some(draft) = &mut self.scope {
                draft.dirty = false;
            }
        }
        self.pending_replay = matches!(action, WorkspaceAction::Replay { .. });
        self.last_request = Some(context.request_id.clone());
        self.delivery = None;
        self.recovered_pending = false;
        self.pending = Some(context.clone());
        self.status = Some(OperationOutcome::Running);
        Some(WorkspaceRequest { context, action })
    }
    /// Call once per egui frame with the controller's current view. Consume at
    /// most one returned request, rotate `next_operation_id` immediately (also
    /// during a running job), and publish replies with the exact request context.
    /// Cancel/Back/Close replace the pending context, fencing later old replies.
    pub fn show(&mut self, ui: &mut Ui, view: &WorkspaceView) -> WorkspaceOutput {
        self.sync(view);
        // egui applies Escape/Tab focus movement before widgets are rendered.
        // Keep the preceding text focus so a shortcut cannot escape an editor.
        let keyboard_allowed = !self.text_focused && !text_edit_focused(ui.ctx());
        let mut output = WorkspaceOutput::default();
        let trace = &mut output.trace;
        // Consume registered shortcuts and repeat activation before the host's
        // Back/Close/Cancel buttons can interpret Enter or Space as a click.
        let keyboard_side = self
            .comparison_keyboard_side
            .as_deref()
            .unwrap_or("before")
            .to_owned();
        let keyboard_runtime = view
            .comparison
            .as_ref()
            .map(|comparison| {
                if keyboard_side == "after" {
                    &comparison.after.runtime
                } else {
                    &comparison.before.runtime
                }
            })
            .or_else(|| view.active.as_ref().map(|active| &active.runtime))
            .filter(|_| self.change.is_none() && self.scope.is_none() && view.adoption.is_none());
        let keyboard_view = keyboard_runtime.and_then(|runtime| {
            runtime
                .program
                .views
                .iter()
                .find(|item| item.id == runtime.observation.view)
        });
        let shortcuts = take_shortcuts(ui, keyboard_view, keyboard_allowed, trace);
        let available =
            self.pending.is_none() && self.last_request.as_ref() != Some(&view.next_operation_id);
        let can_interrupt = self.last_request.as_ref() != Some(&view.next_operation_id);
        let mut action = None;
        let mut interruption = None;
        let identity_stale = self.witness.is_some() && self.witness_basis != self.identity;
        let plan_stale = self
            .plan_basis
            .as_ref()
            .is_some_and(|(_, basis)| basis != &self.identity);
        ScrollArea::vertical().id_salt("product-workspace").show(ui,|ui| {
            ui.set_max_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if (view.active.is_some() || view.comparison.is_some() || self.change.is_some()) && trace.button(ui,"back","Back to work",can_interrupt) { self.change=None; self.scope=None; interruption=Some(WorkspaceAction::Back); }
                if view.active.is_some() && trace.button(ui,"close","Close tool",can_interrupt) { self.change=None; interruption=Some(WorkspaceAction::Close); }
                if let Some(pending)=&self.pending {
                    if trace.button(ui,"cancel","Cancel operation",can_interrupt) { interruption=Some(WorkspaceAction::Cancel{operation:pending.request_id.clone()}); }
                }
            });
            if let Some(status)=&self.status { trace.label(ui,status_text(status)); }
            if view.active.is_none() && view.comparison.is_none() {
                trace.label(ui,"What do you want this tool to help you do?");
                trace.control("need",ui.add(TextEdit::multiline(&mut self.need).id_salt("product-need").desired_width(ui.available_width()).desired_rows(4)));
                ui.horizontal_wrapped(|ui| {
                    if trace.button(ui,"create","Create a tool",available && !self.need.trim().is_empty() && self.need.len()<=MAX_TEXT_BYTES) { action=Some(WorkspaceAction::Create{need:self.need.clone()}); }
                    if trace.button(ui,"open","Open a saved tool…",available) { action=Some(WorkspaceAction::Open); }
                });
                trace.label(ui,"Describe familiar work. The next step checks available generation and any required permission.");
            } else if let Some(comparison)=&view.comparison {
                trace.label(ui,&comparison.question);
                trace.label(ui,"Both sides start with the same data and receive the same inputs. These copies do not change your saved work.");
                for unknown in &comparison.unknowns { trace.label(ui,format!("Not checked: {unknown}")); }
                if comparison.stale || identity_stale { trace.label(ui,"The source or work changed. Restart with a fresh comparison before choosing an outcome."); }
                if let Some(certificate)=&comparison.minimization {
                    trace.label(ui,if certificate.complete {"1-minimal under the listed reductions; not a global minimum"} else {"Smallest found so far; minimization is incomplete"});
                    for operation in &certificate.deletion_operations { trace.label(ui,operation); }
                } else { trace.label(ui,"Minimization has not been checked"); }
                let ready=comparison_ready(comparison) && !identity_stale;
                if self.scope.is_none() && view.adoption.is_none() {
                    ui.horizontal_wrapped(|ui| {
                        if trace.button(ui,"restart","Restart both copies",available) { action=Some(WorkspaceAction::RestartComparison{witness:comparison.witness_id.clone()}); }
                        if trace.button(ui,"replay","Play next input on both",available && !self.before.pending_edits() && !self.after.pending_edits() && !comparison.stale && !identity_stale && comparison.replay_index<comparison.inputs.len()) { action=Some(WorkspaceAction::Replay{witness:comparison.witness_id.clone(),input:comparison.inputs[comparison.replay_index].clone()}); }
                    });
                    trace.label(ui,format!("Input {} of {}",comparison.replay_index.min(comparison.inputs.len()),comparison.inputs.len()));
                    ui.collapsing("Input sequence",|ui| for (index,input) in comparison.inputs.iter().enumerate() { trace.label(ui,format!("{}. {}",index+1,input_text(input))); });
                }
                // At narrow widths stack complete copies rather than clipping a
                // wide table. Both layouts use the exact same runtime renderer.
                let play=available && ready && self.scope.is_none() && view.adoption.is_none();
                let mut side_action_taken=action.is_some();
                let mut side_drafts_pending=false;
                let locked_editor = if self.pending.is_some() || self.before.pending_edits() || self.after.pending_edits() { self.comparison_editor.clone() } else { None };
                let block_editing = self.pending.is_some() && (!self.pending_replay || self.comparison_editor.is_none());
                let mut render_side=|ui:&mut Ui, side:&OutcomeView, renderer:&mut ProductRuntimeView, prefix:&str, label:&str| {
                    trace.label(ui,label); outcome_status(ui,trace,side);
                    let mut runtime=side.runtime.clone(); runtime.read_only|=!ready || block_editing || locked_editor.as_deref().is_some_and(|editor| editor!=prefix) || self.recovered_pending || self.scope.is_some() || view.adoption.is_some();
                    let keyboard_here=if keyboard_side==prefix {shortcuts.as_slice()} else {&[]};
                    let rendered=renderer.show(ui,&runtime,prefix,play && !side_action_taken,false,keyboard_here);
                    if rendered.trace.interacted { self.comparison_keyboard_side=Some(prefix.into()); }
                    side_drafts_pending|=rendered.pending_edits;
                    if rendered.pending_edits { self.comparison_editor=Some(prefix.into()); }
                    trace.append(rendered.trace);
                    if let Some(input)=rendered.input { self.comparison_editor=Some(prefix.into()); side_action_taken=true; action=Some(WorkspaceAction::Replay{witness:comparison.witness_id.clone(),input}); }
                };
                if ui.available_width()>=720.0 {
                    ui.columns(2,|columns| { render_side(&mut columns[0],&comparison.before,&mut self.before,"before","Current outcome"); render_side(&mut columns[1],&comparison.after,&mut self.after,"after","Alternative outcome"); });
                } else {
                    ui.group(|ui|render_side(ui,&comparison.before,&mut self.before,"before","Current outcome"));
                    ui.group(|ui|render_side(ui,&comparison.after,&mut self.after,"after","Alternative outcome"));
                }
                if self.scope.is_none() && view.adoption.is_none() {
                    ui.separator(); trace.label(ui,"Which consequence works for you?");
                    for (key,label,outcome) in [
                        ("choice.before","Use the current outcome",DecisionOutcome::Accept{artifact:comparison.before.artifact.program_digest.clone()}),
                        ("choice.after","Use the alternative outcome",DecisionOutcome::Accept{artifact:comparison.after.artifact.program_digest.clone()}),
                        ("choice.either","Either is acceptable",DecisionOutcome::EitherAcceptable),
                        ("choice.both","I need both behaviors",DecisionOutcome::BothNeeded),
                        ("choice.neither","Neither works",DecisionOutcome::NeitherFits),
                        ("choice.defer","Decide later",DecisionOutcome::Deferred),
                        ("choice.keep","Keep my current tool",DecisionOutcome::KeepCurrent),
                    ] {
                        let dismissal=matches!(outcome,DecisionOutcome::Deferred|DecisionOutcome::KeepCurrent);
                        if trace.button(ui,key,label,available && action.is_none() && (!side_drafts_pending || dismissal) && (ready || dismissal)) { action=Some(WorkspaceAction::Choose{witness:comparison.witness_id.clone(),outcome}); }
                    }
                    trace.label(ui,"Both needed, neither, and later preserve pending intent; they do not activate a behavior.");
                }
            } else if let Some(active)=&view.active {
                if let Some(change)=&mut self.change {
                    trace.label(ui,"What should work differently here?");
                    trace.label(ui,"Your current view, selection and recent inputs will accompany this request locally. External sharing requires the controller's permission check.");
                    trace.control("change.text",ui.add(TextEdit::multiline(change).id_salt("product-change").desired_width(ui.available_width()).desired_rows(4)));
                    if trace.button(ui,"change.submit","Try this change",available && !change.trim().is_empty() && change.len()<=MAX_TEXT_BYTES) { action=Some(WorkspaceAction::Change{utterance:change.clone()}); }
                } else {
                    let rendered=self.daily.show(ui,&active.runtime,"daily",available,true,&shortcuts);
                    trace.append(rendered.trace);
                    if let Some(input)=rendered.input { action=Some(WorkspaceAction::Daily{input}); }
                    if let Some(artifact)=rendered.export { action=Some(WorkspaceAction::Export{artifact}); }
                    ui.separator();
                    if trace.button(ui,"change","This isn't right; change how it works",available && action.is_none() && !rendered.pending_edits) { self.change=Some(String::new()); }
                    for unavailable in &active.capabilities.unavailable { trace.label(ui,format!("Unavailable: {unavailable}")); }
                    for decision in &active.decisions { ui.collapsing(&decision.request,|ui| {
                        trace.label(ui,format!("{} · {:?}",outcome_text(&decision.outcome),decision.status));
                        if let Some(rationale)=&decision.rationale { trace.label(ui,rationale); }
                        scope_summary(ui,trace,&decision.scope);
                    }); }
                }
            }
            if let Some(draft)=&mut self.scope {
                ui.separator(); trace.label(ui,format!("Chosen consequence: {}",outcome_text(&draft.source.outcome)));
                trace.label(ui,"Where should this choice apply?");
                for impact in &draft.source.impact { trace.label(ui,impact); }
                trace.label(ui,format!("Scope rehearsal: {}",evidence_text(draft.source.rehearsal_state)));
                if draft.scope!=draft.source.proposed_scope || normalized_rationale(&draft.rationale)!=normalized_rationale(draft.source.rationale.as_deref().unwrap_or_default()) { trace.label(ui,"Scope changed. A new rehearsal is required before adoption."); }
                let mut operations=std::collections::BTreeMap::new();
                for runtime in view.active.iter().map(|active|&active.runtime).chain(view.comparison.iter().flat_map(|comparison|[&comparison.before.runtime,&comparison.after.runtime])) {
                    for operation in &runtime.program.actions { operations.entry(operation.id.clone()).or_insert_with(||operation.label.clone()); }
                }
                for operation in draft.scope.operations.iter().chain(&draft.source.proposed_scope.operations) { operations.entry(operation.clone()).or_insert_with(||operation.clone()); }
                for (operation,label) in operations {
                    let mut selected=draft.scope.operations.contains(&operation);
                    if trace.control(&format!("scope.operation.{operation}"),ui.checkbox(&mut selected,format!("{label} ({operation})"))).changed() { draft.dirty=true; if selected {draft.scope.operations.insert(operation);} else {draft.scope.operations.remove(&operation);} }
                }
                if let Some(active)=&view.active {
                    let selected=active.runtime.observation.selected.clone();
                    for (key,label,population,enabled) in [
                        ("scope.population.new","New work only",Population::NewWork,true),
                        ("scope.population.selected","Selected existing records",Population::Records{records:selected.clone()},!selected.is_empty()),
                        ("scope.population.all","All work, including existing records",Population::All,true),
                    ] {
                        if trace.control(key,ui.add_enabled(enabled,egui::RadioButton::new(draft.scope.population==population,label))).clicked() { draft.scope.population=population; draft.dirty=true; }
                    }
                }
                // Structured predicates, exclusions and unresolved boundaries are
                // retained exactly; this is not a manual JSON editing surface.
                scope_summary(ui,trace,&draft.scope);
                trace.label(ui,"Reason (optional)");
                if trace.control("scope.rationale",ui.add(TextEdit::multiline(&mut draft.rationale).id_salt("product-rationale").desired_width(ui.available_width()).desired_rows(2))).changed() { draft.dirty=true; }
                let valid=draft.scope.validate().is_ok() && draft.rationale.len()<=MAX_TEXT_BYTES;
                let fresh=view.comparison.as_ref().is_some_and(|c|!c.stale && !identity_stale && c.witness_id==draft.source.witness_id);
                if trace.button(ui,"scope.preview","Rehearse this scope",available && valid && fresh) { action=Some(WorkspaceAction::SetScope{choice:draft.source.choice_id.clone(),scope:draft.scope.clone(),rationale:normalized_rationale(&draft.rationale).map(str::to_owned)}); }
            }
            if let Some(adoption)=&view.adoption {
                ui.separator(); trace.label(ui,"Review the checked change before keeping it");
                for issue in adoption.issues.iter().chain(&adoption.compatibility.issues) { trace.label(ui,issue); }
                trace.label(ui,format!("Current data compatibility: {:?}",adoption.compatibility.state));
                for check in &adoption.checks { trace.label(ui,format!("{:?}: {}",check.state,check.explanation)); }
                for decision in &adoption.retired_decisions { trace.label(ui,format!("This choice will supersede: {decision}")); }
                let scope_edited=self.scope.as_ref().is_some_and(|d|d.scope!=d.source.proposed_scope || normalized_rationale(&d.rationale)!=normalized_rationale(d.source.rationale.as_deref().unwrap_or_default()));
                let stale=identity_stale || plan_stale || view.comparison.as_ref().is_some_and(|c|c.stale);
                if trace.button(ui,"adopt","Keep this checked change",available && action.is_none() && adoption.can_confirm && !scope_edited && !stale) { action=Some(WorkspaceAction::Adopt{plan:adoption.plan_id.clone()}); }
            }
        });
        self.text_focused = text_edit_focused(ui.ctx());
        if let Some(action) = interruption.or(action) {
            output.request = self.request(view, action);
        }
        output
    }
}
fn comparison_ready(view: &ComparisonView) -> bool {
    !view.stale
        && [&view.before, &view.after]
            .iter()
            .all(|side| side.state == EvidenceState::Observed && side.evidence.is_some())
}
fn normalized_rationale(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}
fn operation_pending(outcome: &OperationOutcome) -> bool {
    matches!(
        outcome,
        OperationOutcome::Running | OperationOutcome::AwaitingConsent(_)
    )
}
fn evidence_text(state: EvidenceState) -> &'static str {
    match state {
        EvidenceState::Observed => "Observed",
        EvidenceState::NoDifferenceFound => "No difference found in checked inputs",
        EvidenceState::Inconclusive => "Not established",
        EvidenceState::Unsupported => "Unsupported; not checked",
        EvidenceState::Stale => "Stale; rerun required",
        EvidenceState::Failed => "Check failed",
    }
}
fn outcome_status(ui: &mut Ui, trace: &mut WidgetTrace, side: &OutcomeView) {
    trace.label(ui, evidence_text(side.state));
    trace.label(
        ui,
        match side.origin {
            ExecutionOrigin::ProductionRuntime => "Production runtime observation",
            ExecutionOrigin::LegacyRuntime => "Legacy runtime observation",
            ExecutionOrigin::TestFixture => "Test fixture; not live generation evidence",
        },
    );
    if side.evidence.is_none() {
        trace.label(
            ui,
            "Unverified suggestion: no execution evidence is attached",
        );
    }
    for uncovered in &side.uncovered {
        trace.label(ui, format!("Not checked: {uncovered}"));
    }
}
fn status_text(status: &OperationOutcome) -> String {
    match status {
        OperationOutcome::Running => "Working…".into(),
        OperationOutcome::PreviewReady => "Preview ready".into(),
        OperationOutcome::AwaitingScope => "Choose where this applies".into(),
        OperationOutcome::AwaitingConsent(why) => format!("Permission needed: {why}"),
        OperationOutcome::Committed { generation } => format!("Saved (revision {generation})"),
        OperationOutcome::Rejected(why) => format!("Could not apply this: {why}"),
        OperationOutcome::Retryable(why) => format!("Could not finish. You can try again: {why}"),
        OperationOutcome::Cancelled => "Operation cancelled".into(),
    }
}
fn outcome_text(outcome: &DecisionOutcome) -> &'static str {
    match outcome {
        DecisionOutcome::Accept { .. } => "Selected outcome",
        DecisionOutcome::KeepCurrent => "Keep current",
        DecisionOutcome::EitherAcceptable => "Either is acceptable",
        DecisionOutcome::BothNeeded => "Both behaviors needed",
        DecisionOutcome::NeitherFits => "Neither fits",
        DecisionOutcome::Deferred => "Decide later",
    }
}
fn scope_summary(ui: &mut Ui, trace: &mut WidgetTrace, scope: &DecisionScope) {
    trace.label(
        ui,
        format!(
            "Operations: {}",
            scope
                .operations
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    trace.label(
        ui,
        match &scope.population {
            Population::NewWork => "New work only".into(),
            Population::All => "All work, including existing records".into(),
            Population::CreatedAfter { generation } => {
                format!("Work created after revision {generation}")
            }
            Population::Records { records } => {
                format!("{} selected existing records", records.len())
            }
            Population::Entity { entity } => format!("Records in {entity}"),
            Population::Where { entity, predicate } => {
                format!("Records in {entity} where {}", condition_text(predicate))
            }
        },
    );
    if let Population::Records { records } = &scope.population {
        for record in records {
            trace.label(
                ui,
                format!("Included: {} / {}", record.entity, record.record),
            );
        }
    }
    for (condition, value) in &scope.conditions {
        trace.label(ui, format!("{condition}: {}", scope_value_text(value)));
    }
    for record in &scope.excluded_records {
        trace.label(
            ui,
            format!("Excluded: {} / {}", record.entity, record.record),
        );
    }
    for unknown in &scope.unknowns {
        trace.label(ui, format!("Still undecided: {}", unknown.description));
    }
}
/// Describe a typed restriction without evaluating it or replacing its meaning
/// with a model-written summary. Parentheses preserve nested condition grouping.
fn condition_text(condition: &Expr) -> String {
    let describe = condition_text;
    match condition {
        Expr::Literal { value, .. } => scope_value_text(value),
        Expr::Variable { name } => format!("variable {name}"),
        Expr::Field { record, field } => format!("field {field} of ({})", describe(record)),
        Expr::State { state } => format!("current {state}"),
        Expr::Today => "today".into(),
        Expr::Query {
            entity,
            binding,
            predicate,
            sort,
            limit,
            include_archived,
        } => {
            let order = sort
                .iter()
                .map(|key| {
                    format!(
                        "{} {}",
                        describe(&key.value),
                        if key.descending {
                            "descending"
                        } else {
                            "ascending"
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("records in {entity}, called {binding}, where ({}), {} archived records, ordered by [{}], at most {limit}", describe(predicate), if *include_archived { "including" } else { "excluding" }, order)
        }
        Expr::Filter {
            items,
            binding,
            predicate,
        } => format!(
            "({}) filtered to {binding} where ({})",
            describe(items),
            describe(predicate)
        ),
        Expr::Map {
            items,
            binding,
            value,
        } => format!(
            "for each {binding} in ({}), use ({})",
            describe(items),
            describe(value)
        ),
        Expr::Count { items } => format!("count of ({})", describe(items)),
        Expr::Any {
            items,
            binding,
            predicate,
        } => format!(
            "any {binding} in ({}) satisfies ({})",
            describe(items),
            describe(predicate)
        ),
        Expr::All {
            items,
            binding,
            predicate,
        } => format!(
            "every {binding} in ({}) satisfies ({})",
            describe(items),
            describe(predicate)
        ),
        Expr::Sum { items } => format!("sum of ({})", describe(items)),
        Expr::Contains { items, value } => {
            format!("({}) contains ({})", describe(items), describe(value))
        }
        Expr::Equal { left, right } => format!("({}) equals ({})", describe(left), describe(right)),
        Expr::Less { left, right } => {
            format!("({}) is less than ({})", describe(left), describe(right))
        }
        Expr::Add { left, right } => format!("({}) plus ({})", describe(left), describe(right)),
        Expr::Subtract { left, right } => {
            format!("({}) minus ({})", describe(left), describe(right))
        }
        Expr::Concat { left, right } => {
            format!("join text ({}) with ({})", describe(left), describe(right))
        }
        Expr::TextContains { text, search } => {
            format!("text ({}) contains ({})", describe(text), describe(search))
        }
        Expr::Lower { value } => format!("lowercase text ({})", describe(value)),
        Expr::And { values } => format!(
            "all of [{}]",
            values.iter().map(describe).collect::<Vec<_>>().join("; ")
        ),
        Expr::Or { values } => format!(
            "at least one of [{}]",
            values.iter().map(describe).collect::<Vec<_>>().join("; ")
        ),
        Expr::Not { value } => format!("not ({})", describe(value)),
        Expr::If {
            condition,
            then_value,
            else_value,
        } => format!(
            "if ({}), then ({}), otherwise ({})",
            describe(condition),
            describe(then_value),
            describe(else_value)
        ),
        Expr::Coalesce { value, fallback } => format!(
            "({}), or ({}) when not set",
            describe(value),
            describe(fallback)
        ),
        Expr::DateAdd { date, days } => {
            format!("date ({}) plus ({}) days", describe(date), describe(days))
        }
        Expr::DateDifference { later, earlier } => {
            format!("days from ({}) to ({})", describe(earlier), describe(later))
        }
    }
}
fn scope_value_text(value: &DataValue) -> String {
    match value {
        DataValue::Text { value } => format!("{value:?}"),
        DataValue::List { items, .. } => format!(
            "[{}]",
            items
                .iter()
                .map(scope_value_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value_text(value),
    }
}

fn input_text(input: &SemanticInput) -> String {
    match input {
        SemanticInput::Control { control, value, .. } => {
            format!("Set {control} to {}", value_text(value))
        }
        SemanticInput::Activate { binding, .. } => format!("Use {binding}"),
        SemanticInput::Submit { view, .. } => format!("Submit {view}"),
        SemanticInput::Navigate { view } => format!("Open {view}"),
        SemanticInput::Invoke { action, .. } => format!("Run {action}"),
        SemanticInput::AdvanceClock { days } => {
            format!("Move the copied clock forward {days} days")
        }
        SemanticInput::Observe { point } => format!("Inspect {point}"),
    }
}
