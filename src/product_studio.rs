//! Generated-tool host. One worker owns blocking generation, copied changes,
//! verification, adoption and job cleanup; egui only renders bounded models.
#[path = "product_studio/change.rs"]
mod change;
#[path = "product_studio/change_adapter.rs"]
mod change_adapter;
#[path = "product_studio/intention_flow.rs"]
mod intention_flow;
#[path = "product_studio/journal.rs"]
mod journal;
#[path = "product_studio/worker.rs"]
mod worker;
use crate::product_backup::{
    inspect_open, open_verified, upgrade_open_verified, CheckpointShelf, OpenGate,
};
use crate::product_contract::*;
use crate::product_decisions::{DecisionEngine, IntentArchive};
use crate::product_discovery::{
    encode_request, prepare_development, PreparedDevelopment, ProviderOptions,
};
use crate::product_locations::{RecentEntry, RecentTools, ToolIdentity, ToolLocations};
use crate::product_protocol::RuntimeView;
use crate::product_provider::JobReceipt;
use crate::product_provider::{
    CapabilityProfile, ConsentReceipt, DataDisclosure, JobState, ProviderKind, ProviderTransport,
};
use crate::product_runtime::{LocalRuntime, ProductRun};
#[cfg(test)]
use crate::product_store::scope::LifecycleBinding;
use crate::product_store::scope::ScopePopulation;
use crate::product_store::{ProductStore, ProjectSnapshot, UpgradeProgress, UpgradeSummary};
use crate::ui::product_runtime_view::{take_shortcuts, ProductRuntimeView, WidgetTrace};
use change::{ChangeDraft, ChangeView};
use journal::{
    Association, Basis, Interrupted, JournalFile, ProviderAssociation, ReconcileAssociation,
    UnsavedInput,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

fn error(e: impl std::fmt::Debug) -> String {
    let text = format!("{e:?}");
    if text.len() <= MAX_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_TEXT_BYTES - 3;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}
fn id(kind: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{kind}-{:x}-{:x}-{:x}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn day() -> i32 {
    (chrono::Local::now().date_naive() - chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
        .num_days() as i32
}

#[derive(Clone)]
struct Gate {
    state: Arc<AtomicU8>,
    cancelled: Arc<AtomicBool>,
    upgrade_stage: Arc<AtomicU8>,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(0)),
            cancelled: Arc::new(AtomicBool::new(false)),
            upgrade_stage: Arc::new(AtomicU8::new(0)),
        }
    }
}
impl Gate {
    fn cancel(&self) -> bool {
        if self
            .state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.cancelled.store(true, Ordering::Release);
            true
        } else {
            self.state.load(Ordering::Acquire) == 2
        }
    }
    fn commit(&self) -> Result<(), String> {
        self.state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "Cancelled before saving; no business change was committed".into())
    }
    fn finish(&self) -> bool {
        self.state
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            || matches!(self.state.load(Ordering::Acquire), 1 | 3)
    }
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Cancelled; the original need and saved work were kept".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    session: String,
    epoch: u64,
    operation: String,
    basis: Option<Basis>,
}
enum Action {
    Boot,
    Prepare {
        need: String,
        provider: ProviderKind,
        profile: CapabilityProfile,
    },
    Consent {
        disclosure: String,
    },
    Modify {
        need: String,
        provider: ProviderKind,
        profile: CapabilityProfile,
        focused: Option<RecordRef>,
    },
    Trial {
        input: SemanticInput,
    },
    Scope {
        population: ScopePopulation,
    },
    Finished {
        record: RecordRef,
        field: Id,
    },
    NoFinished {
        entity: Id,
    },
    Decide {
        outcome: DecisionOutcome,
    },
    ResumeChoice {
        decision: Id,
    },
    History,
    PrepareReconciliation {
        needs: Vec<Id>,
        need: String,
        provider: ProviderKind,
        profile: CapabilityProfile,
    },
    PreviewWithdrawal {
        layers: Vec<Digest>,
    },
    IntentionTrial {
        input: SemanticInput,
    },
    CommitIntention {
        operation: Id,
    },
    ReturnDaily,
    Select {
        candidate: usize,
    },
    Preview {
        input: SemanticInput,
    },
    Save,
    ChooseDestination,
    Open {
        path: PathBuf,
        expected: Option<ToolIdentity>,
    },
    OpenDialog,
    Upgrade {
        tool: Association,
        summary: UpgradeSummary,
    },
    Daily {
        input: SemanticInput,
    },
    Reconcile,
    AbandonCreation,
    AbandonDaily,
    Tick {
        today: i32,
    },
    Close,
    #[cfg(test)]
    DialogCancelled,
}
struct Command {
    key: Key,
    gate: Gate,
    action: Action,
}
struct Pending {
    key: Key,
    gate: Gate,
    renderer: bool,
    freeze_inputs: bool,
}
#[derive(Clone)]
enum IntentionView {
    Design(intention_flow::DesignView),
    Withdrawal(intention_flow::WithdrawalView),
}
impl IntentionView {
    fn preview(&self) -> &RuntimeView {
        match self {
            Self::Design(v) => &v.preview,
            Self::Withdrawal(v) => &v.preview,
        }
    }
}
#[derive(Clone)]
enum Page {
    Home,
    Upgrade {
        tool: Association,
        summary: UpgradeSummary,
    },
    Consent {
        disclosure: DataDisclosure,
        need: String,
        request: DevelopmentRequest,
        review: String,
        basis: Option<Basis>,
    },
    Change(ChangeView),
    History {
        basis: Basis,
        view: intention_flow::HistoryView,
    },
    Intention {
        basis: Basis,
        view: IntentionView,
        origin: String,
    },
    Draft {
        basis: Basis,
        labels: Vec<String>,
        selected: usize,
        model: RuntimeView,
        origin: String,
        unsupported: Vec<String>,
    },
    Daily {
        tool: Association,
        basis: Basis,
        model: RuntimeView,
        choices: Vec<ScopedDecision>,
    },
}
#[derive(Clone)]
struct Update {
    page: Option<Page>,
    need: Option<String>,
    recent: Vec<RecentEntry>,
    notice: String,
    destination: Option<PathBuf>,
    mutation_blocked: bool,
    generation_blocked: bool,
    pending: bool,
    abandon_creation: bool,
    abandon_daily: bool,
    unsaved: Option<String>,
}
#[derive(Clone)]
struct Completion {
    key: Key,
    update: Update,
    acknowledged: bool,
    committed: Option<PathBuf>,
}
#[derive(Default)]
struct Config {
    root: Option<PathBuf>,
    #[cfg(test)]
    transport: Option<ProviderTransport>,
    #[cfg(test)]
    hooks: TestHooks,
}

pub struct ProductStudio {
    send: mpsc::SyncSender<Command>,
    receive: mpsc::Receiver<Completion>,
    pending: Option<Pending>,
    session: String,
    epoch: u64,
    page: Page,
    renderer: ProductRuntimeView,
    alternative_renderer: ProductRuntimeView,
    scope_selection: Vec<RecordRef>,
    history: intention_flow::HistoryState,
    need: String,
    provider: ProviderKind,
    profile: CapabilityProfile,
    recent: Vec<RecentEntry>,
    destination: Option<PathBuf>,
    notice: String,
    mutation_blocked: bool,
    generation_blocked: bool,
    unresolved: bool,
    closing: bool,
    abandon_creation: bool,
    abandon_daily: bool,
    unsaved: Option<String>,
    clock_attempt: Option<(Basis, i32)>,
    #[cfg(test)]
    clock: Arc<std::sync::atomic::AtomicI32>,
}
impl Default for ProductStudio {
    fn default() -> Self {
        Self::new()
    }
}
impl ProductStudio {
    pub fn new() -> Self {
        Self::construct(Config::default())
    }
    fn construct(config: Config) -> Self {
        let (send, commands) = mpsc::sync_channel(1);
        let (complete, receive) = mpsc::sync_channel(1);
        #[cfg(test)]
        let clock = config.hooks.today.clone();
        std::thread::spawn(move || worker::run(config, commands, complete));
        let mut studio = Self {
            send,
            receive,
            pending: None,
            session: id("session"),
            epoch: 0,
            page: Page::Home,
            renderer: ProductRuntimeView::default(),
            alternative_renderer: ProductRuntimeView::default(),
            scope_selection: vec![],
            history: intention_flow::HistoryState::default(),
            need: String::new(),
            provider: ProviderKind::Codex,
            profile: CapabilityProfile::DataOnly,
            recent: vec![],
            destination: None,
            notice: String::new(),
            mutation_blocked: true,
            generation_blocked: true,
            unresolved: false,
            closing: false,
            abandon_creation: false,
            abandon_daily: false,
            unsaved: None,
            clock_attempt: None,
            #[cfg(test)]
            clock,
        };
        studio.issue(Action::Boot);
        studio
    }
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }
    fn basis(&self) -> Option<Basis> {
        match &self.page {
            Page::Daily { basis, .. } | Page::Draft { basis, .. } => Some(basis.clone()),
            Page::Change(view) => Some(view.basis.clone()),
            Page::History { basis, .. } | Page::Intention { basis, .. } => Some(basis.clone()),
            Page::Consent { basis, .. } => basis.clone(),
            _ => None,
        }
    }
    fn inputs_pending(&self) -> bool {
        self.renderer.pending_edits() || self.alternative_renderer.pending_edits()
    }
    fn action_waits_for_inputs(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::Decide { .. }
                | Action::History
                | Action::PrepareReconciliation { .. }
                | Action::PreviewWithdrawal { .. }
                | Action::CommitIntention { .. }
                | Action::IntentionTrial {
                    input: SemanticInput::AdvanceClock { .. }
                }
                | Action::Modify { .. }
                | Action::ResumeChoice { .. }
                | Action::Scope { .. }
                | Action::Finished { .. }
                | Action::NoFinished { .. }
                | Action::Trial {
                    input: SemanticInput::AdvanceClock { .. }
                }
        ) && self.inputs_pending()
    }
    fn plain_trial_control(&self, input: &SemanticInput) -> bool {
        let SemanticInput::Control { view, control, .. } = input else {
            return false;
        };
        let plain = |model: &RuntimeView| {
            if model.observation.view != *view {
                return false;
            }
            model
                .program
                .views
                .iter()
                .find(|v| &v.id == view)
                .is_some_and(|v| {
                    let ViewKind::List {
                        controls,
                        selection,
                        ..
                    } = &v.kind
                    else {
                        return false;
                    };
                    controls
                        .iter()
                        .find(|c| &c.id == control)
                        .map(|c| c.on_change.is_none())
                        .or_else(|| {
                            selection
                                .as_ref()
                                .filter(|s| &s.id == control)
                                .map(|s| s.on_change.is_none())
                        })
                        .unwrap_or(false)
                })
        };
        match &self.page {
            Page::Change(change) => {
                plain(&change.current) && change.alternative.as_ref().is_none_or(plain)
            }
            Page::Intention { view, .. } => plain(view.preview()),
            _ => false,
        }
    }
    fn issue(&mut self, action: Action) {
        if self.pending.is_some() {
            return;
        }
        if self.action_waits_for_inputs(&action) {
            self.notice = "Finish or correct the inputs before continuing.".into();
            return;
        }
        let key = Key {
            session: self.session.clone(),
            epoch: self.epoch,
            operation: id("operation"),
            basis: self.basis(),
        };
        let gate = Gate::default();
        // Initialization must finish even if the user immediately changes panels.
        if matches!(action, Action::Boot) {
            let _ = gate.commit();
        }
        let renderer = matches!(
            action,
            Action::Preview { .. }
                | Action::Daily { .. }
                | Action::Trial { .. }
                | Action::IntentionTrial { .. }
        );
        // Actions and navigation can replace a copied view or prepared source.
        // Only plain controls on both actual sources keep this editing frame.
        let freeze_inputs = match &action {
            Action::Trial { input } => !self.plain_trial_control(input),
            Action::IntentionTrial { input } => !self.plain_trial_control(input),
            Action::Daily {
                input: SemanticInput::Navigate { .. },
            } => true,
            Action::Preview { .. } | Action::Daily { .. } => false,
            _ => true,
        };
        match self.send.try_send(Command { key: key.clone(), gate: gate.clone(), action }) {
            Ok(()) => self.pending = Some(Pending { key, gate, renderer, freeze_inputs }),
            Err(_) => self.notice = "The tool worker is unavailable. Saved work was kept; reopen the app to reconcile unfinished work".into(),
        }
    }
    /// Called by App before selecting any panel, so navigation never abandons
    /// provider cleanup, durable acknowledgements, or pending store work.
    pub fn poll(&mut self) {
        while let Ok(completion) = self.receive.try_recv() {
            if self.pending.as_ref().map(|p| &p.key) != Some(&completion.key) {
                continue;
            }
            let renderer_request = self.pending.as_ref().is_some_and(|p| p.renderer);
            self.pending = None;
            let fresh =
                completion.key.session == self.session && completion.key.epoch == self.epoch;
            let update = completion.update;
            self.recent = update.recent;
            self.mutation_blocked = update.mutation_blocked;
            self.generation_blocked = update.generation_blocked;
            self.unresolved = update.pending;
            self.abandon_creation = update.abandon_creation;
            self.abandon_daily = update.abandon_daily;
            self.unsaved = update.unsaved;
            self.destination = update.destination;
            if fresh && !self.closing {
                if let Some(page) = update.page {
                    let reset = std::mem::discriminant(&self.page) != std::mem::discriminant(&page);
                    self.page = page;
                    if reset {
                        self.renderer = ProductRuntimeView::default();
                        self.alternative_renderer = ProductRuntimeView::default();
                        self.scope_selection.clear();
                        if matches!(self.page, Page::History { .. }) {
                            self.history = intention_flow::HistoryState::default();
                        }
                    }
                }
                if let Some(need) = update.need {
                    self.need = need;
                }
                if renderer_request {
                    self.renderer.acknowledge(completion.acknowledged);
                    self.alternative_renderer
                        .acknowledge(completion.acknowledged);
                }
                self.notice = update.notice;
            } else {
                self.notice = match completion.committed {
                    Some(path) => format!(
                        "Your work was saved at {} before closing. {}",
                        path.display(),
                        update.notice
                    ),
                    None => update.notice,
                };
            }
            if self.closing {
                self.closing = false;
                self.issue(Action::Close);
            }
        }
        // Clock changes are ordinary receipt-bound store operations, not a
        // different rendering clock. Retry failures only on explicit request.
        if !self.is_busy() && !self.mutation_blocked && !self.closing {
            if let Page::Daily { basis, .. } = &self.page {
                let today = self.today();
                let attempt = (basis.clone(), today);
                if basis.day < today && self.clock_attempt.as_ref() != Some(&attempt) {
                    self.clock_attempt = Some(attempt);
                    self.issue(Action::Tick { today });
                }
            }
        }
    }
    fn today(&self) -> i32 {
        #[cfg(test)]
        {
            self.clock.load(Ordering::Acquire)
        }
        #[cfg(not(test))]
        {
            day()
        }
    }
    fn cancel(&mut self) -> bool {
        let accepted = self.pending.as_ref().is_some_and(|p| p.gate.cancel());
        self.notice = if accepted { "Cancellation requested; waiting for the worker to confirm" } else { "This operation has reached its completion boundary and cannot be cancelled. Waiting for the actual result" }.into();
        accepted
    }
    fn close(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.page = Page::Home;
        self.renderer = ProductRuntimeView::default();
        self.alternative_renderer = ProductRuntimeView::default();
        if self.pending.is_some() {
            self.cancel();
            self.closing = true;
        } else {
            self.issue(Action::Close);
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) -> WidgetTrace {
        let mut trace = WidgetTrace::default();
        let active_model = match &self.page {
            Page::Draft { model, .. } | Page::Daily { model, .. } => Some(model),
            Page::Change(view) => Some(&view.current),
            Page::Intention { view, .. } => Some(view.preview()),
            _ => None,
        };
        let shortcuts = take_shortcuts(
            ui,
            active_model.and_then(|m| m.program.views.iter().find(|v| v.id == m.observation.view)),
            !self.is_busy() && !self.mutation_blocked,
            &mut trace,
        );
        let today = self.today();
        let busy = self.is_busy();
        let inputs_pending = self.inputs_pending();
        let inputs_frozen = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.freeze_inputs);
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
        trace.label(ui, "Your generated tools");
        trace.label(ui, "Create a local tool from your own need. Saved tools work offline; try changes on copies before keeping them.");
        if !self.notice.is_empty() {
            trace.label(ui, self.notice.clone());
        }
        if let Some(unsaved) = &self.unsaved {
            ui.collapsing("Last unsaved entry (kept for reference)", |ui| {
                trace.label(ui, unsaved.clone());
            });
        }
        let mut action = None;
        let mut close = false;
        if busy {
            trace.label(ui, "Working… You can continue using the other app panels.");
            match self
                .pending
                .as_ref()
                .map(|p| p.gate.upgrade_stage.load(Ordering::Acquire))
            {
                Some(1) => trace.label(ui, "Upgrade: validating the original saved tool…"),
                Some(2) => trace.label(ui, "Upgrade: staged locally; original data is kept…"),
                Some(3) => trace.label(ui, "Upgrade: saved records and history verified…"),
                Some(4) => trace.label(
                    ui,
                    "Upgrade: activating the verified format; do not close the app…",
                ),
                Some(5) => trace.label(
                    ui,
                    "Upgrade verified. Restarting the tool with a fresh open session…",
                ),
                _ => (),
            }
            if trace.button(
                ui,
                "studio.cancel",
                "Cancel",
                self.pending
                    .as_ref()
                    .is_some_and(|p| p.gate.state.load(Ordering::Acquire) == 0),
            ) {
                self.cancel();
            }
        }
        if self.unresolved {
            trace.label(ui, "An interrupted operation needs reconciliation. Unresolved saves pause new writes; unresolved provider requests pause generation. Nothing is automatically resubmitted.");
            if trace.button(
                ui,
                "studio.reconcile",
                "Check or retry the exact unfinished save",
                !busy,
            ) {
                action = Some(Action::Reconcile);
            }
            if self.abandon_daily {
                trace.label(ui, "You may set aside an input only after its current saved tool is verified to have no matching commit. The original input stays available for reference.");
                if trace.button(
                    ui,
                    "studio.abandon-input",
                    "Set aside this unsaved input",
                    !busy,
                ) {
                    action = Some(Action::AbandonDaily);
                }
            }
            if self.abandon_creation {
                trace.label(ui, "If the original folder has no committed tool, you can set this attempt aside. Its files are kept. An existing or unreadable CURRENT is never discarded.");
                if trace.button(
                    ui,
                    "studio.abandon",
                    "Set aside this uncommitted save",
                    !busy,
                ) {
                    action = Some(Action::AbandonCreation);
                }
            }
        }
        match &self.page {
            Page::Upgrade { tool, summary } => {
                trace.label(ui, "This saved generated tool needs a local format upgrade before you can continue working.");
                trace.label(
                    ui,
                    format!(
                        "Format {} → {} · {} records · {} events · {} outputs",
                        summary.from_version,
                        summary.to_version,
                        summary.records,
                        summary.events,
                        summary.artifacts
                    ),
                );
                trace.label(ui, "Original snapshots and backups are kept. This runs offline, verifies saved work, then restarts the tool in a fresh session. Nothing is sent to an AI service.");
                trace.label(ui, format!("Saved locally: {}", tool.path.display()));
                if trace.button(
                    ui,
                    "studio.upgrade",
                    "Upgrade, restart tool and reopen",
                    !busy,
                ) {
                    action = Some(Action::Upgrade {
                        tool: tool.clone(),
                        summary: summary.clone(),
                    });
                }
                close = trace.button(ui, "studio.back", "Back without upgrading", true);
            }
            Page::Home => {
                trace.label(ui, "What would you like this tool to help you do?");
                trace.control(
                    "studio.need",
                    ui.add_enabled(
                        !busy,
                        egui::TextEdit::multiline(&mut self.need)
                            .desired_rows(3)
                            .char_limit(MAX_TEXT_BYTES),
                    ),
                );
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.provider, ProviderKind::Codex, "Codex");
                    ui.selectable_value(&mut self.provider, ProviderKind::Claude, "Claude");
                });
                ui.add_enabled_ui(!busy, |ui| {
                    ui.radio_value(
                        &mut self.profile,
                        CapabilityProfile::DataOnly,
                        "Data-only (requires verified availability)",
                    );
                    ui.radio_value(
                        &mut self.profile,
                        CapabilityProfile::TrustedHarness,
                        "Trusted Harness (broader access; review before each request)",
                    );
                });
                trace.label(ui, "Data-only isolation is not attested for installed providers. Trusted Harness may access broader host tools or context; a login is not permission to share data. No request is sent before you review its prepared disclosure.");
                if trace.button(
                    ui,
                    "studio.prepare",
                    "Review generation request",
                    !busy
                        && !self.generation_blocked
                        && !self.need.trim().is_empty()
                        && self.need.len() <= MAX_TEXT_BYTES,
                ) {
                    action = Some(Action::Prepare {
                        need: self.need.clone(),
                        provider: self.provider,
                        profile: self.profile,
                    });
                }
                if trace.button(ui, "studio.open", "Open a saved tool…", !busy) {
                    action = Some(Action::OpenDialog);
                }
                trace.label(ui, "Recent tools");
                for entry in &self.recent {
                    let label = format!("{} ({:?})", entry.tool.label, entry.availability);
                    if trace.button(
                        ui,
                        &format!("studio.recent.{}", entry.tool.id.as_str()),
                        &label,
                        !busy,
                    ) {
                        action = Some(Action::Open {
                            path: entry.tool.path.clone(),
                            expected: Some(entry.tool.identity.clone()),
                        });
                    }
                }
            }
            Page::Consent {
                disclosure,
                need,
                request,
                review,
                ..
            } => {
                trace.label(
                    ui,
                    format!("Send this request to {}?", disclosure.recipient),
                );
                trace.label(ui, format!("Your exact need: {need}"));
                if request.operation == DevelopmentOperation::Generate {
                    trace.label(ui, "This initial request includes the need above and the public application-language instructions/schema. It contains no saved tool sources, business records, earlier conversations, or accepted scenes.");
                } else {
                    trace.label(ui, format!("This request includes {} exact tool sources, the current view and selection, {} recent acknowledged inputs, {} retained intentions, and {} complete inherited examples with their business records and observed results. These copies may contain private information; they are not automatically sanitized.", request.sources.len(), request.context.recent_inputs.len(), request.decisions.decisions.len(), request.examples.len()));
                    ui.collapsing("Review every value sent in this exact request", |ui| {
                        trace.label(ui,"This read-only payload includes the exact request, sources, selected records, recent inputs, full inherited scenarios and accepted observations. You do not need to edit it.");
                        egui::ScrollArea::vertical().max_height(360.0).show(ui,|ui| {
                            let mut text=review.as_str();
                            trace.control("studio.disclosure-payload",ui.add(egui::TextEdit::multiline(&mut text).interactive(false).desired_width(f32::INFINITY)));
                        });
                    });
                }
                trace.label(ui, disclosure.purpose.clone());
                for category in &disclosure.data_categories {
                    trace.label(ui, category.clone());
                }
                trace.label(ui, disclosure.capability_notice.clone());
                trace.label(ui, disclosure.billing_notice.clone());
                trace.label(
                    ui,
                    format!(
                        "Prepared profile: {:?}. Request: {}",
                        disclosure.profile, disclosure.request_id
                    ),
                );
                if trace.button(
                    ui,
                    "studio.consent",
                    "Allow this exact request and generate",
                    !busy && !self.generation_blocked,
                ) {
                    action = Some(Action::Consent {
                        disclosure: disclosure.digest(),
                    });
                }
                if request.operation != DevelopmentOperation::Generate {
                    if trace.button(
                        ui,
                        "studio.return",
                        "Back to saved work without sending",
                        !busy,
                    ) {
                        action = Some(Action::ReturnDaily);
                    }
                } else {
                    close = trace.button(ui, "studio.back", "Back without sending", true);
                }
            }
            Page::History { basis, view } => {
                let (event, widgets) = self.history.show(
                    ui,
                    view,
                    !busy && !self.mutation_blocked && basis.day == today,
                );
                trace.append(widgets);
                if let Some(event) = event {
                    action = Some(match event {
                        intention_flow::Event::Reconcile { needs, request } => {
                            Action::PrepareReconciliation {
                                needs,
                                need: request,
                                provider: self.provider,
                                profile: self.profile,
                            }
                        }
                        intention_flow::Event::PreviewWithdrawal { layers } => {
                            Action::PreviewWithdrawal { layers }
                        }
                        _ => Action::ReturnDaily,
                    });
                }
                if basis.day != today
                    && trace.button(
                        ui,
                        "studio.return",
                        "Return to saved work to update today's date",
                        !busy,
                    )
                {
                    action = Some(Action::ReturnDaily);
                }
            }
            Page::Intention {
                basis,
                view,
                origin,
            } => {
                trace.label(ui, origin.clone());
                let can_commit =
                    !busy && !inputs_pending && !self.mutation_blocked && basis.day == today;
                let (event, widgets) = match view {
                    IntentionView::Design(view) => view.show(ui, !busy, can_commit),
                    IntentionView::Withdrawal(view) => view.show(ui, !busy, can_commit),
                };
                trace.append(widgets);
                if let Some(event) = event {
                    action = Some(match event {
                        intention_flow::Event::AcceptDesign { operation }
                        | intention_flow::Event::Withdraw { operation } => {
                            Action::CommitIntention { operation }
                        }
                        _ => Action::ReturnDaily,
                    });
                }
                if inputs_pending {
                    trace.label(ui, "Finish or correct the copied inputs before saving this change, or explicitly discard them and return.");
                }
                if basis.day != today {
                    trace.label(ui, "Today's date changed. Return to saved work and prepare a fresh preview before saving.");
                }
                let mut model = std::borrow::Cow::Borrowed(view.preview());
                if inputs_frozen || basis.day != today {
                    model.to_mut().read_only = true;
                }
                let output = self.renderer.show(
                    ui,
                    &model,
                    "intention-copy",
                    !busy && action.is_none(),
                    false,
                    &shortcuts,
                );
                trace.append(output.trace);
                if action.is_none() {
                    action = output.input.map(|input| Action::IntentionTrial { input });
                }
            }
            Page::Draft {
                labels,
                selected,
                model,
                origin,
                unsupported,
                ..
            } => {
                trace.label(ui, "Isolated draft: try fictional work here. Keeping the tool starts with empty real records; draft entries are not saved business work.");
                trace.label(ui, origin.clone());
                for item in unsupported {
                    trace.label(ui, format!("Provider reported an unsupported part: {item}"));
                }
                for (index, label) in labels.iter().enumerate() {
                    if trace.button(
                        ui,
                        &format!("studio.candidate.{index}"),
                        &format!(
                            "{}{}",
                            if index == *selected {
                                "Previewing: "
                            } else {
                                "Try: "
                            },
                            label
                        ),
                        !busy && index != *selected,
                    ) {
                        action = Some(Action::Select { candidate: index });
                    }
                }
                if let Some(destination) = &self.destination {
                    trace.label(ui, format!("Data location: {}", destination.display()));
                }
                if trace.button(ui, "studio.destination", "Choose a data folder…", !busy) {
                    action = Some(Action::ChooseDestination);
                }
                if trace.button(
                    ui,
                    "studio.keep",
                    "Keep this tool and start real work",
                    !busy && !self.mutation_blocked,
                ) {
                    action = Some(Action::Save);
                }
                close = trace.button(ui, "studio.back", "Back without keeping", true);
                ui.separator();
                let output = self.renderer.show(
                    ui,
                    model,
                    "draft",
                    !busy && action.is_none() && !close,
                    false,
                    &shortcuts,
                );
                trace.append(output.trace);
                if action.is_none() {
                    if let Some(input) = output.input {
                        action = Some(Action::Preview { input });
                    }
                }
            }
            Page::Change(view) => {
                trace.label(ui, format!("Requested change: {}", view.need));
                trace.label(ui, view.origin.clone());
                trace.label(ui, "Copied work only. Both sides use the same starting records, date and input sequence. Trial entries never become saved business work.");
                trace.label(ui, format!(
                    "Copied date: {}. Advancing this date changes only the copies, never saved work or its date.",
                    crate::ui::product_runtime_view::value_text(&DataValue::Date { days: view.trial_day })
                ));
                if trace.button(
                    ui,
                    "studio.trial.next-day",
                    "Try one day later on the copies",
                    !busy && !inputs_pending,
                ) {
                    action = Some(Action::Trial {
                        input: SemanticInput::AdvanceClock { days: 1 },
                    });
                }
                if !view.lifecycle_note.is_empty() {
                    trace.label(ui, view.lifecycle_note.clone());
                }
                if !view.missing.is_empty() {
                    trace.label(ui, "When should this work count as finished, so future rule changes leave its recorded result alone? Try the existing completion action below, then choose the recorded outcome that defines finished work.");
                    for option in &view.lifecycle_options {
                        if trace.button(
                            ui,
                            &format!(
                                "studio.finished.{}.{}.{}",
                                option.record.entity, option.record.record, option.field
                            ),
                            &option.label,
                            !busy && !inputs_pending,
                        ) {
                            action = Some(Action::Finished {
                                record: option.record.clone(),
                                field: option.field.clone(),
                            });
                        }
                    }
                    for (entity, label) in &view.missing {
                        if trace.button(
                            ui,
                            &format!("studio.no-finished.{entity}"),
                            &format!("{label} has no finished state"),
                            !busy && !inputs_pending,
                        ) {
                            action = Some(Action::NoFinished {
                                entity: entity.clone(),
                            });
                        }
                    }
                } else {
                    if view.needs_task {
                        trace.label(ui,"Try an actual task below before recording a choice. A copied view alone does not define which work the choice applies to; both versions will be independently replayed.");
                    }
                    if view.whole_design {
                        trace.label(ui, "This changes the tool's design as a whole. Existing protected results and intentions are independently checked; partial rule scopes are unavailable for this design.");
                    } else {
                        trace.label(ui, "Choose where the rule applies. Already completed results and past events remain preserved, including with All work.");
                        for (key, label, population) in [
                            (
                                "studio.scope.future",
                                "Future work",
                                ScopePopulation::FutureWork,
                            ),
                            ("studio.scope.all", "All work", ScopePopulation::All),
                        ] {
                            if trace.button(
                                ui,
                                key,
                                label,
                                !busy && !inputs_pending && view.population != population,
                            ) {
                                action = Some(Action::Scope { population });
                            }
                        }
                        ui.collapsing("Choose these unfinished items", |ui| {
                            for row in &view.scope_rows {
                                if row.archived {
                                    continue;
                                }
                                let reference = RecordRef {
                                    entity: row.entity.clone(),
                                    record: row.id.clone(),
                                };
                                let label = row
                                    .values
                                    .values()
                                    .find_map(|v| {
                                        if let DataValue::Text { value } = v {
                                            Some(value.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_else(|| row.entity.clone());
                                let mut selected = self.scope_selection.contains(&reference);
                                let response = ui.add_enabled(
                                    !busy && !inputs_pending,
                                    egui::Checkbox::new(&mut selected, label),
                                );
                                if response.changed() {
                                    if selected {
                                        self.scope_selection.push(reference.clone());
                                    } else {
                                        self.scope_selection.retain(|r| r != &reference);
                                    }
                                }
                                trace.control(
                                    &format!("studio.select.{}.{}", row.entity, row.id),
                                    response,
                                );
                            }
                            if trace.button(
                                ui,
                                "studio.scope.selected",
                                "Apply to these unfinished items",
                                !busy
                                    && !inputs_pending
                                    && view.partial_scope
                                    && !self.scope_selection.is_empty(),
                            ) {
                                action = Some(Action::Scope {
                                    population: ScopePopulation::SelectedUnfinished {
                                        records: self.scope_selection.clone(),
                                    },
                                });
                            }
                        });
                    }
                    trace.label(
                        ui,
                        format!(
                            "Current scope: {}",
                            match &view.population {
                                ScopePopulation::All => "All work".into(),
                                ScopePopulation::FutureWork =>
                                    "Future work begun after acceptance".into(),
                                ScopePopulation::SelectedUnfinished { records } =>
                                    format!("{} frozen selected unfinished items", records.len()),
                            }
                        ),
                    );
                    trace.label(
                        ui,
                        format!(
                            "Try the affected tasks before accepting: {}",
                            view.operations.join(", ")
                        ),
                    );
                    if !view.unavailable_tasks.is_empty() {
                        trace.label(ui, format!(
                            "These required tasks cannot be tried through controls on both sides: {}. This comparison cannot verify the full chosen scope. Return to saved work to keep the existing tool and any earlier choice; a revised design with usable controls is needed before acceptance.",
                            view.unavailable_tasks.join(", ")
                        ));
                    }
                    for note in &view.readiness_notes {
                        trace.label(ui, note.clone());
                    }
                    if trace.button(
                        ui,
                        "studio.accept",
                        if view.wording_update {
                            "Apply checked wording update"
                        } else {
                            "Accept the alternative"
                        },
                        !busy && !inputs_pending && view.can_accept,
                    ) {
                        action = Some(Action::Decide {
                            outcome: DecisionOutcome::Accept {
                                artifact: view.target_artifact.clone(),
                            },
                        });
                    }
                    for (key, label, outcome) in [
                        (
                            "studio.keep-current",
                            "Keep current",
                            DecisionOutcome::KeepCurrent,
                        ),
                        (
                            "studio.either",
                            "Either is acceptable",
                            DecisionOutcome::EitherAcceptable,
                        ),
                        ("studio.both", "I need both", DecisionOutcome::BothNeeded),
                        (
                            "studio.neither",
                            "Neither fits",
                            DecisionOutcome::NeitherFits,
                        ),
                        ("studio.defer", "Decide later", DecisionOutcome::Deferred),
                    ] {
                        if trace.button(
                            ui,
                            key,
                            label,
                            !busy
                                && !inputs_pending
                                && if outcome == DecisionOutcome::KeepCurrent {
                                    view.can_keep_current
                                } else {
                                    view.can_retain
                                },
                        ) {
                            action = Some(Action::Decide { outcome });
                        }
                    }
                    if inputs_pending {
                        trace.label(
                            ui,
                            "Finish or correct the copied inputs below before changing scope, advancing the date or recording a choice.",
                        );
                    }
                    if view.same_alternative {
                        trace.label(ui,"Both sides use the same rules. A new pair will not be recorded. Returning to saved work leaves any earlier choices unchanged.");
                    } else {
                        trace.label(ui,"The four unresolved choices retain both actual experiences without changing your live tool. You can reopen them after continuing work.");
                    }
                }
                if inputs_pending {
                    trace.label(ui, "Returning discards unfinished copied edits. Saved work and earlier choices stay unchanged.");
                }
                if trace.button(
                    ui,
                    "studio.return",
                    if inputs_pending {
                        "Discard copied edits and return"
                    } else {
                        "Return to saved work"
                    },
                    !busy,
                ) {
                    action = Some(Action::ReturnDaily);
                }
                ui.separator();
                if let Some(alternative) = &view.alternative {
                    ui.columns(2, |columns| {
                        trace.label(&mut columns[0], "Current");
                        let mut current = std::borrow::Cow::Borrowed(&view.current);
                        if inputs_frozen || self.alternative_renderer.pending_edits() {
                            current.to_mut().read_only = true;
                        }
                        let output = self.renderer.show(
                            &mut columns[0],
                            &current,
                            "current",
                            !busy && action.is_none(),
                            false,
                            &shortcuts,
                        );
                        trace.append(output.trace);
                        let mut current_dispatched = false;
                        if let Some(input) = output.input {
                            if action.is_none() {
                                current_dispatched = true;
                                action = Some(Action::Trial { input });
                            }
                        }
                        trace.label(
                            &mut columns[1],
                            if view.same_alternative {
                                "Same rules"
                            } else {
                                "Alternative"
                            },
                        );
                        let mut alternative = std::borrow::Cow::Borrowed(alternative);
                        // Both panes drive one input sequence. Claim this frame
                        // before the other pane can start a competing edit.
                        if inputs_frozen || current_dispatched || self.renderer.pending_edits() {
                            alternative.to_mut().read_only = true;
                        }
                        let output = self.alternative_renderer.show(
                            &mut columns[1],
                            &alternative,
                            "alternative",
                            !busy && action.is_none(),
                            false,
                            &[],
                        );
                        trace.append(output.trace);
                        if let Some(input) = output.input {
                            if action.is_none() {
                                action = Some(Action::Trial { input });
                            }
                        }
                    });
                } else {
                    let mut current = std::borrow::Cow::Borrowed(&view.current);
                    if inputs_frozen {
                        current.to_mut().read_only = true;
                    }
                    let output = self.renderer.show(
                        ui,
                        &current,
                        "current",
                        !busy && action.is_none(),
                        false,
                        &shortcuts,
                    );
                    trace.append(output.trace);
                    if let Some(input) = output.input {
                        if action.is_none() {
                            action = Some(Action::Trial { input });
                        }
                    }
                }
            }
            Page::Daily {
                tool,
                basis,
                model,
                choices,
            } => {
                trace.label(
                    ui,
                    format!(
                        "{} · saved revision {} · business generation {}",
                        model.program.label, basis.revision, basis.data_generation
                    ),
                );
                trace.label(ui, format!("Saved locally: {}", tool.path.display()));
                trace.label(ui, "Output files cannot be saved in this initial version. Generated output can be inspected below.");
                close = trace.button(ui, "studio.close", "Close tool", true);
                if trace.button(
                    ui,
                    "studio.history",
                    "Saved ways of working and rule history",
                    !busy && !inputs_pending && !self.mutation_blocked && basis.day == today,
                ) {
                    action = Some(Action::History);
                }
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.provider, ProviderKind::Codex, "Codex");
                        ui.selectable_value(&mut self.provider, ProviderKind::Claude, "Claude");
                    });
                    ui.radio_value(
                        &mut self.profile,
                        CapabilityProfile::DataOnly,
                        "Data-only (requires verified availability)",
                    );
                    ui.radio_value(
                        &mut self.profile,
                        CapabilityProfile::TrustedHarness,
                        "Trusted Harness (review broader access before sending)",
                    );
                });
                trace.label(ui, "What should work differently here?");
                trace.control(
                    "studio.change-need",
                    ui.add_enabled(
                        !busy,
                        egui::TextEdit::multiline(&mut self.need)
                            .desired_rows(2)
                            .char_limit(MAX_TEXT_BYTES),
                    ),
                );
                if trace.button(
                    ui,
                    "studio.modify",
                    "Review this change request",
                    !busy
                        && !inputs_pending
                        && !self.generation_blocked
                        && !self.need.trim().is_empty()
                        && basis.day == today,
                ) {
                    action = Some(Action::Modify {
                        need: self.need.clone(),
                        provider: self.provider,
                        profile: self.profile,
                        focused: self.renderer.focused_record().cloned(),
                    });
                }
                for choice in choices
                    .iter()
                    .filter(|d| d.status == DecisionStatus::Pending)
                {
                    trace.label(
                        ui,
                        format!(
                            "Saved unresolved choice: {} · {:?}",
                            choice.request, choice.outcome
                        ),
                    );
                    if trace.button(
                        ui,
                        &format!("studio.resume.{}", choice.id),
                        "Try this saved choice on current work",
                        !busy && !inputs_pending && !self.mutation_blocked,
                    ) {
                        action = Some(Action::ResumeChoice {
                            decision: choice.id.clone(),
                        });
                    }
                }
                if inputs_pending {
                    trace.label(ui, "Finish, correct or discard the work inputs below before requesting a change or revisiting a choice. Closing the tool discards unfinished edits.");
                }
                let mut model = model.clone();
                model.read_only |= inputs_frozen || self.mutation_blocked || basis.day != today;
                if basis.day != today {
                    trace.label(ui, "The saved tool date differs from today. Current-day work is paused until its date is safely updated; the saved data is kept.");
                    if basis.day < today
                        && trace.button(
                            ui,
                            "studio.today",
                            "Update to today",
                            !busy && !self.mutation_blocked,
                        )
                    {
                        action = Some(Action::Tick { today });
                    }
                }
                let output = self.renderer.show(
                    ui,
                    &model,
                    "daily",
                    !busy && !self.mutation_blocked && action.is_none() && !close,
                    false,
                    &shortcuts,
                );
                trace.append(output.trace);
                if action.is_none() {
                    if let Some(input) = output.input {
                        action = Some(Action::Daily { input });
                    }
                }
            }
        }
        if close {
            self.close();
        } else if let Some(action) = action {
            // Editing stays available while an input is pending. A control can also
            // enqueue a new value later in this frame, after a choice button.
            let waiting_for_inputs = self.action_waits_for_inputs(&action);
            self.issue(action);
            if waiting_for_inputs {
                ui.ctx().request_repaint();
            }
        }
        trace
    }
}
impl Drop for ProductStudio {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.gate.cancel();
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct TestPause {
    pub reached: AtomicBool,
    pub release: AtomicBool,
}
#[cfg(test)]
#[derive(Clone, Copy)]
pub enum TestProviderAssociationFault {
    Missing,
    DifferentRequest,
}
#[cfg(test)]
#[derive(Clone)]
pub struct TestHooks {
    pub before_commit: Option<Arc<TestPause>>,
    pub before_preview: Option<Arc<TestPause>>,
    pub before_context_transition: Option<Arc<TestPause>>,
    pub before_return_install: Option<Arc<TestPause>>,
    pub provider_association_fault: Option<TestProviderAssociationFault>,
    pub before_destination: Option<Arc<TestPause>>,
    pub before_abandon: Option<Arc<TestPause>>,
    pub fail_creation: Arc<AtomicBool>,
    pub folder_choice: Option<PathBuf>,
    pub after_commit: Option<Arc<TestPause>>,
    pub lose_ack: Arc<AtomicBool>,
    pub stopped: Arc<AtomicBool>,
    pub duplicate_completion: bool,
    pub today: Arc<std::sync::atomic::AtomicI32>,
}
#[cfg(test)]
impl Default for TestHooks {
    fn default() -> Self {
        Self {
            before_commit: None,
            before_preview: None,
            before_context_transition: None,
            before_return_install: None,
            provider_association_fault: None,
            before_destination: None,
            before_abandon: None,
            fail_creation: Arc::new(AtomicBool::new(false)),
            folder_choice: None,
            after_commit: None,
            lose_ack: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            duplicate_completion: false,
            today: Arc::new(std::sync::atomic::AtomicI32::new(20000)),
        }
    }
}
#[cfg(test)]
impl ProductStudio {
    pub fn testing(root: PathBuf, transport: Option<ProviderTransport>, hooks: TestHooks) -> Self {
        Self::construct(Config {
            root: Some(root),
            transport,
            hooks,
        })
    }
    pub fn test_page(&self) -> &'static str {
        match self.page {
            Page::Home => "home",
            Page::Upgrade { .. } => "upgrade",
            Page::Consent { .. } => "consent",
            Page::Draft { .. } => "draft",
            Page::Daily { .. } => "daily",
            Page::Change(_) => "change",
            Page::History { .. } => "history",
            Page::Intention { ref view, .. } => match view {
                IntentionView::Design(_) => "design",
                IntentionView::Withdrawal(_) => "withdrawal",
            },
        }
    }
    pub fn test_modify(&mut self, need: &str) {
        self.issue(Action::Modify {
            need: need.into(),
            provider: self.provider,
            profile: self.profile,
            focused: self.renderer.focused_record().cloned(),
        });
    }
    pub fn test_consent(&mut self) {
        if let Page::Consent { disclosure, .. } = &self.page {
            self.issue(Action::Consent {
                disclosure: disclosure.digest(),
            });
        }
    }
    pub fn test_disclosure_payload(&self) -> Option<&str> {
        if let Page::Consent { review, .. } = &self.page {
            Some(review)
        } else {
            None
        }
    }
    pub fn test_prepared_request(&self) -> Option<&DevelopmentRequest> {
        if let Page::Consent { request, .. } = &self.page {
            Some(request)
        } else {
            None
        }
    }
    pub fn test_lifecycle_outcome(&mut self, record: RecordRef, field: &str) {
        self.issue(Action::Finished {
            record,
            field: field.into(),
        });
    }
    pub fn test_scope(&mut self, population: ScopePopulation) {
        self.issue(Action::Scope { population });
    }
    pub fn test_trial(&mut self, input: SemanticInput) {
        self.issue(Action::Trial { input });
    }
    pub fn test_decide(&mut self, outcome: DecisionOutcome) {
        let outcome = if matches!(outcome, DecisionOutcome::Accept { .. }) {
            if let Page::Change(view) = &self.page {
                DecisionOutcome::Accept {
                    artifact: view.target_artifact.clone(),
                }
            } else {
                outcome
            }
        } else {
            outcome
        };
        self.issue(Action::Decide { outcome });
    }
    pub fn test_decide_exact(&mut self, outcome: DecisionOutcome) {
        self.issue(Action::Decide { outcome });
    }
    pub fn test_resume_choice(&mut self, decision: &str) {
        self.issue(Action::ResumeChoice {
            decision: decision.into(),
        });
    }
    pub fn test_can_accept(&self) -> bool {
        matches!(&self.page,Page::Change(v) if v.can_accept)
    }
    pub fn test_scope_records(&self) -> Vec<RecordRef> {
        if let Page::Change(view) = &self.page {
            view.scope_rows
                .iter()
                .map(|r| RecordRef {
                    entity: r.entity.clone(),
                    record: r.id.clone(),
                })
                .collect()
        } else {
            vec![]
        }
    }
    pub fn test_current_trial(&self) -> Option<&RuntimeView> {
        if let Page::Change(v) = &self.page {
            Some(&v.current)
        } else {
            None
        }
    }
    pub fn test_alternative(&self) -> Option<&RuntimeView> {
        if let Page::Change(v) = &self.page {
            v.alternative.as_ref()
        } else {
            None
        }
    }
    pub fn test_notice(&self) -> &str {
        &self.notice
    }
    pub fn test_need(&self) -> &str {
        &self.need
    }
    pub fn test_runtime(&self) -> Option<&RuntimeView> {
        match &self.page {
            Page::Draft { model, .. } | Page::Daily { model, .. } => Some(model),
            Page::Intention { view, .. } => Some(view.preview()),
            _ => None,
        }
    }
    pub fn test_location(&self) -> Option<&Path> {
        match &self.page {
            Page::Daily { tool, .. } => Some(&tool.path),
            _ => None,
        }
    }
    pub fn test_open(&mut self, path: PathBuf) {
        self.issue(Action::Open {
            path,
            expected: None,
        });
    }
    pub fn test_daily(&mut self, input: SemanticInput) {
        self.issue(Action::Daily { input });
    }
    pub fn test_upgrade(&mut self) {
        if let Page::Upgrade { tool, summary } = &self.page {
            self.issue(Action::Upgrade {
                tool: tool.clone(),
                summary: summary.clone(),
            });
        }
    }
    pub fn test_cancel(&mut self) -> bool {
        self.cancel()
    }
    pub fn test_close(&mut self) {
        self.close();
    }
    pub fn test_abandon_daily(&mut self) {
        self.issue(Action::AbandonDaily);
    }
    pub fn test_destination(&self) -> Option<&Path> {
        self.destination.as_deref()
    }
    pub fn test_abandon(&mut self) {
        self.issue(Action::AbandonCreation);
    }
    pub fn test_generation_blocked(&self) -> bool {
        self.generation_blocked
    }
    pub fn test_reconcile(&mut self) {
        self.issue(Action::Reconcile);
    }
    pub fn test_choose_open(&mut self, path: Option<PathBuf>) {
        match path {
            Some(path) => self.test_open(path),
            None => self.issue(Action::DialogCancelled),
        }
    }
}

#[cfg(test)]
pub fn test_stage_daily(root: &Path, path: &Path, input: SemanticInput, commit: bool) {
    let opened = open_verified(path, None).unwrap();
    let tool = Association {
        path: path.into(),
        identity: ToolIdentity::from_snapshot(&opened.snapshot).unwrap(),
    };
    let basis = Basis::capture(&opened.snapshot).unwrap();
    let mut journal = JournalFile::open(root).unwrap();
    let mut value = journal.value.clone();
    value.last = Some(tool.clone());
    value.pending = Some(Interrupted::Daily {
        tool,
        basis: basis.clone(),
        operation: "interrupted-input".into(),
        input: input.clone(),
    });
    journal.write(value).unwrap();
    if commit {
        opened
            .store
            .apply(
                basis.revision,
                "interrupted-input",
                &input,
                RuntimeLimits::default(),
            )
            .unwrap();
    }
}
#[cfg(test)]
pub fn test_stage_creation(root: &Path, path: &Path) {
    let opened = open_verified(path, None).unwrap();
    let tool = Association {
        path: path.into(),
        identity: ToolIdentity::from_snapshot(&opened.snapshot).unwrap(),
    };
    let mut journal = JournalFile::open(root).unwrap();
    let mut value = journal.value.clone();
    value.pending = Some(Interrupted::Create { tool });
    journal.write(value).unwrap();
}
#[cfg(test)]
pub fn test_stage_provider(
    root: &Path,
    request: DevelopmentRequest,
    provider: ProviderKind,
    profile: CapabilityProfile,
) {
    let wire = encode_request(
        &request,
        &ProviderOptions {
            provider,
            profile,
            ..ProviderOptions::default()
        },
    )
    .unwrap();
    let mut journal = JournalFile::open(root).unwrap();
    let mut value = journal.value.clone();
    value.need = request.request.clone();
    value.provider = Some(ProviderAssociation {
        request,
        provider,
        profile,
        wire_request: wire.digest().unwrap(),
        wire_source: wire.source_digest,
        issued: true,
        modify: None,
        reconcile: None,
    });
    journal.write(value).unwrap();
}

#[cfg(test)]
pub fn test_stage_unstarted_creation(root: &Path, path: &Path, capture: &CapturedProgram) {
    let tool = Association {
        path: path.into(),
        identity: ToolIdentity {
            project_id: capture.binding.project_id.clone(),
            first_program: canonical_digest(IdentityDomain::Source, capture).unwrap(),
        },
    };
    let mut journal = JournalFile::open(root).unwrap();
    let mut value = journal.value.clone();
    value.pending = Some(Interrupted::Create { tool });
    journal.write(value).unwrap();
}

#[cfg(test)]
pub fn test_scope_request(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    population: ScopePopulation,
    lifecycles: Vec<LifecycleBinding>,
) -> Result<crate::product_store::scope::ScopeRequest, String> {
    change_adapter::request(
        snapshot,
        candidate,
        population,
        lifecycles,
        std::collections::BTreeSet::new(),
    )
}

#[cfg(test)]
pub fn test_stage_change(root: &Path, path: &Path, before: &ProjectSnapshot, plan: AdoptionPlan) {
    let tool = Association {
        path: path.into(),
        identity: ToolIdentity::from_snapshot(before).unwrap(),
    };
    let mut journal = JournalFile::open(root).unwrap();
    let mut value = journal.value.clone();
    value.last = Some(tool.clone());
    value.pending = Some(Interrupted::Change {
        tool,
        basis: Basis::capture(before).unwrap(),
        plan,
    });
    journal.write(value).unwrap();
}
