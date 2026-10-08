//! Reachable initial generated-tool host. Later change/scope flows are not yet
//! exposed here. One worker owns every blocking operation, including job Drop.
#[path = "product_studio/journal.rs"]
mod journal;
#[path = "product_studio/worker.rs"]
mod worker;
use crate::product_backup::{
    inspect_open, open_verified, upgrade_open_verified, CheckpointShelf, OpenGate,
};
use crate::product_contract::*;
use crate::product_discovery::{
    encode_request, prepare_development, PreparedDevelopment, ProviderOptions,
};
use crate::product_locations::{RecentEntry, RecentTools, ToolIdentity, ToolLocations};
use crate::product_protocol::RuntimeView;
use crate::product_provider::{
    CapabilityProfile, ConsentReceipt, DataDisclosure, JobState, ProviderKind, ProviderTransport,
};
use crate::product_runtime::{LocalRuntime, ProductRun};
use crate::product_store::{ProductStore, ProjectSnapshot, UpgradeProgress, UpgradeSummary};
use crate::ui::product_runtime_view::{take_shortcuts, ProductRuntimeView, WidgetTrace};
use journal::{Association, Basis, Interrupted, JournalFile, ProviderAssociation, UnsavedInput};
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
            _ => None,
        }
    }
    fn issue(&mut self, action: Action) {
        if self.pending.is_some() {
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
        let renderer = matches!(action, Action::Preview { .. } | Action::Daily { .. });
        match self.send.try_send(Command { key: key.clone(), gate: gate.clone(), action }) {
            Ok(()) => self.pending = Some(Pending { key, gate, renderer }),
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
                    }
                }
                if let Some(need) = update.need {
                    self.need = need;
                }
                if renderer_request {
                    self.renderer.acknowledge(completion.acknowledged);
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
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
        trace.label(ui, "Your generated tools");
        trace.label(ui, "Create a local tool from your own need. Saved tools work offline. Changes to a saved tool are not available in this initial version.");
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
            Page::Consent { disclosure, need } => {
                trace.label(
                    ui,
                    format!("Send this request to {}?", disclosure.recipient),
                );
                trace.label(ui, format!("Your exact need: {need}"));
                trace.label(ui, "This initial request includes the need above and the public application-language instructions/schema. It contains no saved tool sources, business records, earlier conversations, or accepted scenes.");
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
                close = trace.button(ui, "studio.back", "Back without sending", true);
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
            Page::Daily { tool, basis, model } => {
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
                let mut model = model.clone();
                model.read_only |= self.mutation_blocked || basis.day != today;
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
            self.issue(action);
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
#[derive(Clone)]
pub struct TestHooks {
    pub before_commit: Option<Arc<TestPause>>,
    pub before_preview: Option<Arc<TestPause>>,
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
