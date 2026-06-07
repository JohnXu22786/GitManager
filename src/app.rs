use crate::git_ops::*;
use crate::recent::RecentRepos;
use crate::updater::{self, UpdateState};
use eframe::egui;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Tracks a Git operation running in a background thread.
struct PendingOp {
    description: String,
    receiver: mpsc::Receiver<OpResult>,
    started_at: Instant,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Tab {
    Status,
    Branches,
    Worktrees,
    Log,
    Stash,
    Remotes,
}

pub struct App {
    pub git: GitRepo,
    pub current_tab: Tab,
    pub repo_path: String,
    pub error_message: String,
    pub success_message: String,

    pub status_entries: Vec<StatusEntry>,
    pub branches: Vec<BranchInfo>,
    pub worktrees: Vec<WorktreeInfo>,
    pub commits: Vec<CommitInfo>,
    pub stashes: Vec<StashEntry>,
    pub remote_list: Vec<RemoteInfo>,

    pub commit_msg: String,
    pub commit_amend: bool,

    pub branch_filter: String,
    pub new_branch_name: String,
    pub new_branch_base: String,
    pub rename_branch_old: String,
    pub rename_branch_new: String,
    pub merge_branch_name: String,

    pub new_worktree_path: String,
    pub new_worktree_branch: String,
    pub new_worktree_name: String,
    pub new_worktree_create_branch: bool,

    pub stash_message: String,
    pub remote_name: String,
    pub push_branch: String,
    pub push_force: bool,
    pub pull_rebase: bool,

    pub diff_content: Vec<DiffLine>,
    pub diff_path: String,
    pub show_diff: bool,
    pub log_search: String,

    pub last_refresh: std::time::Instant,

    pub show_about: bool,
    pub font_size: f32,

    pub update_state: Arc<Mutex<UpdateState>>,
    pub show_update_dialog: bool,
    pub auto_check_done: bool,
    /// Pending Git operations running in background threads.
    pending_ops: Vec<PendingOp>,
    /// Accumulated error messages from background operations.
    pending_errors: Vec<String>,
    /// Accumulated success messages from background operations.
    pending_successes: Vec<String>,
    /// Whether to auto-refresh after a mutation operation completes.
    needs_refresh: bool,
    pub recent_repos: RecentRepos,
    pub status_expanded: bool,
}

impl App {
    const MIN_FONT_SIZE: f32 = 10.0;
    const MAX_FONT_SIZE: f32 = 24.0;

    pub fn new() -> Self {
        Self {
            git: GitRepo::new(),
            current_tab: Tab::Status,
            repo_path: String::new(),
            error_message: String::new(),
            success_message: String::new(),

            status_entries: Vec::new(),
            branches: Vec::new(),
            worktrees: Vec::new(),
            commits: Vec::new(),
            stashes: Vec::new(),
            remote_list: Vec::new(),

            commit_msg: String::new(),
            commit_amend: false,

            branch_filter: String::new(),
            new_branch_name: String::new(),
            new_branch_base: String::new(),
            rename_branch_old: String::new(),
            rename_branch_new: String::new(),
            merge_branch_name: String::new(),

            new_worktree_path: String::new(),
            new_worktree_branch: String::new(),
            new_worktree_name: String::new(),
            new_worktree_create_branch: false,

            stash_message: String::new(),
            remote_name: String::new(),
            push_branch: String::new(),
            push_force: false,
            pull_rebase: false,

            diff_content: Vec::new(),
            diff_path: String::new(),
            show_diff: false,
            log_search: String::new(),

            last_refresh: std::time::Instant::now(),

            show_about: false,
            font_size: 14.0,

            update_state: Arc::new(Mutex::new(UpdateState::Idle)),
            show_update_dialog: false,
            auto_check_done: false,
            pending_ops: Vec::new(),
            pending_errors: Vec::new(),
            pending_successes: Vec::new(),
            needs_refresh: false,
            recent_repos: RecentRepos::load(),
            status_expanded: false,
        }
    }

    pub fn trigger_update_check(&mut self) {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let state = self.update_state.clone();
        *state.lock().unwrap() = UpdateState::Checking;

        std::thread::spawn(move || {
            let result = updater::check_for_update(&current_version);
            *state.lock().unwrap() = result;
        });
    }

    pub fn open_repo(&mut self, path: &str) {
        self.error_message.clear();
        self.success_message.clear();
        match self.git.open(Path::new(path)) {
            Ok(()) => {
                self.repo_path = path.to_string();
                self.success_message = format!("Opened repository at {}", path);
                self.recent_repos.add(path);
                self.refresh_all();
            }
            Err(e) => {
                self.error_message = format!("Failed to open repo: {}", e);
            }
        }
    }

    /// Checks if there are any pending background operations.
    pub fn is_busy(&self) -> bool {
        !self.pending_ops.is_empty()
    }

    /// Returns the description of the current/last operation.
    pub fn current_operation(&self) -> String {
        self.pending_ops.first()
            .map(|op| op.description.clone())
            .unwrap_or_default()
    }

    /// Spawn a Git operation in a background thread.
    /// Returns immediately. Results will be processed in `process_pending_ops()`.
    pub fn start_operation(&mut self, ctx: &egui::Context, description: &str, op: GitOperation) {
        // Get the repo path to pass to the thread
        let repo_path = match self.git.path() {
            Some(p) => p.to_path_buf(),
            None => {
                self.show_error("No repository open".into());
                return;
            }
        };

        let (tx, rx) = mpsc::channel::<OpResult>();
        let desc = description.to_string();

        std::thread::spawn(move || {
            let result = execute_operation(&repo_path, op);
            let _ = tx.send(result);
        });

        self.pending_ops.push(PendingOp {
            description: desc,
            receiver: rx,
            started_at: Instant::now(),
        });

        ctx.request_repaint();
    }

    /// Process completed background operations.
    /// Must be called at the start of each `update()` frame.
    pub fn process_pending_ops(&mut self, ctx: &egui::Context) {
        let mut i = 0;
        while i < self.pending_ops.len() {
            let op = &self.pending_ops[i];
            // Check if the operation has timed out (60 seconds)
            if op.started_at.elapsed().as_secs() > 60 {
                self.pending_errors
                    .push(format!("Operation '{}' timed out", op.description));
                self.pending_ops.swap_remove(i);
                continue;
            }

            match op.receiver.try_recv() {
                Ok(result) => {
                    let op = self.pending_ops.swap_remove(i);
                    self.handle_op_result(op.description, result);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1; // Still pending
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Thread panicked or channel broken
                    self.pending_errors.push(format!(
                        "Operation '{}' failed unexpectedly",
                        op.description
                    ));
                    self.pending_ops.swap_remove(i);
                }
            }
        }

        // Trigger async refresh after mutation operations complete
        if self.needs_refresh && self.pending_ops.is_empty() {
            self.needs_refresh = false;
            if self.git.is_open() {
                self.start_operation(ctx, "Refreshing", GitOperation::RefreshAll);
            }
        }
    }

    fn handle_op_result(&mut self, description: String, result: OpResult) {
        match result {
            OpResult::Success(msg) => {
                self.pending_successes.push(msg);
                // Auto-refresh after mutation operations
                self.needs_refresh = true;
            }
            OpResult::Error(e) => {
                self.pending_errors.push(format!("{}: {}", description, e));
            }
            OpResult::DiffContent { path, lines } => {
                self.diff_path = path;
                self.diff_content = lines;
                self.show_diff = true;
            }
            OpResult::SearchResults(commits) => {
                self.commits = commits;
            }
            OpResult::RefreshData {
                status_entries,
                branches,
                worktrees,
                commits,
                stashes,
                remote_list,
                errors,
            } => {
                self.status_entries = status_entries;
                self.branches = branches;
                self.worktrees = worktrees;
                self.commits = commits;
                self.stashes = stashes;
                self.remote_list = remote_list;
                self.last_refresh = Instant::now();
                if !errors.is_empty() {
                    self.pending_errors.push(errors.join("; "));
                }
            }
        }
    }

    /// Flush accumulated messages from background operations into the UI message bar.
    pub fn flush_messages(&mut self) {
        if !self.pending_successes.is_empty() {
            self.success_message = self.pending_successes.join(" | ");
            self.pending_successes.clear();
        }
        if !self.pending_errors.is_empty() {
            let msg = self.pending_errors.join(" | ");
            self.pending_errors.clear();
            // Prepend to existing error if any
            if self.error_message.is_empty() {
                self.error_message = msg;
            } else {
                self.error_message = format!("{} | {}", self.error_message, msg);
            }
        }
    }

    pub fn refresh_all(&mut self) {
        if !self.git.is_open() {
            return;
        }
        self.error_message.clear();
        self.success_message.clear();

        // Perform each operation with error reporting instead of silent swallowing
        let mut errors: Vec<String> = Vec::new();

        self.status_entries = self.git.get_status().unwrap_or_else(|e| {
            errors.push(format!("Status: {}", e));
            Vec::new()
        });
        self.branches = self.git.branches().unwrap_or_else(|e| {
            errors.push(format!("Branches: {}", e));
            Vec::new()
        });
        self.worktrees = self.git.worktrees().unwrap_or_else(|e| {
            errors.push(format!("Worktrees: {}", e));
            Vec::new()
        });
        self.commits = self.git.log(100).unwrap_or_else(|e| {
            errors.push(format!("Log: {}", e));
            Vec::new()
        });
        self.stashes = self.git.stash_list().unwrap_or_else(|e| {
            errors.push(format!("Stash: {}", e));
            Vec::new()
        });
        self.remote_list = self.git.remotes().unwrap_or_else(|e| {
            errors.push(format!("Remotes: {}", e));
            Vec::new()
        });

        if !errors.is_empty() {
            self.show_error(errors.join("; "));
        }

        self.last_refresh = std::time::Instant::now();
    }

    pub fn show_error(&mut self, msg: String) {
        self.error_message = msg;
    }

    pub fn show_success(&mut self, msg: String) {
        self.success_message = msg;
    }

    pub fn status_color_by_type(s: char) -> egui::Color32 {
        match s {
            'M' | 'A' | 'R' => egui::Color32::GREEN,
            'D' => egui::Color32::RED,
            '?' | '!' => egui::Color32::GRAY,
            'U' => egui::Color32::YELLOW,
            _ => egui::Color32::GRAY,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-check for updates on startup (first frame only)
        if !self.auto_check_done {
            self.auto_check_done = true;
            self.trigger_update_check();
            ctx.request_repaint();
        }

        // Poll update state from background thread
        {
            let state = self.update_state.lock().unwrap();
            match &*state {
                UpdateState::UpdateAvailable { latest_version, download_url: _ } => {
                    if !self.show_update_dialog {
                        self.show_update_dialog = true;
                    }
                    let _ = latest_version;
                }
                UpdateState::Checking => {
                    ctx.request_repaint();
                }
                _ => {}
            }
        }

        // --- Phase 1: Process any completed background operations ---
        self.process_pending_ops(ctx);
        self.flush_messages();

        let dark = ctx.style().visuals.dark_mode;
        if dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Apply font size via text styles
        let fs = self.font_size;
        ctx.style_mut(|style| {
            style.text_styles = [
                (egui::TextStyle::Body, egui::FontId::proportional(fs)),
                (egui::TextStyle::Button, egui::FontId::proportional(fs)),
                (egui::TextStyle::Heading, egui::FontId::proportional(fs + 4.0)),
                (egui::TextStyle::Small, egui::FontId::proportional(fs - 2.0)),
                (egui::TextStyle::Monospace, egui::FontId::monospace(fs)),
            ].into();
        });

        // --- Top Bar ---
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.is_busy(), egui::Button::new("📂")).clicked() {
                    let path = crate::native_file_dialog();
                    if let Some(p) = path {
                        self.open_repo(&p);
                    }
                }

                // Recent repos dropdown
                egui::menu::menu_button(ui, "▼", |ui| {
                    if self.recent_repos.is_empty() {
                        ui.label("No recent repositories");
                    } else {
                        let mut to_delete: Option<usize> = None;
                        let entries = self.recent_repos.entries().to_vec();
                        ui.label(
                            egui::RichText::new("Recent Repositories")
                                .strong()
                                .size(14.0),
                        );
                        ui.separator();
                        for (i, entry) in entries.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.set_min_width(300.0);
                                if ui
                                    .selectable_label(false, &entry.name)
                                    .clicked()
                                {
                                    self.open_repo(&entry.path);
                                    ui.close_menu();
                                }
                                ui.label(
                                    egui::RichText::new(&entry.path)
                                        .size(10.0)
                                        .color(egui::Color32::GRAY),
                                );
                                if ui.button("🗑").clicked() {
                                    to_delete = Some(i);
                                }
                            });
                        }
                        if let Some(idx) = to_delete {
                            self.recent_repos.remove(idx);
                        }
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("v{}", crate::version_info::VERSION))
                            .color(egui::Color32::GRAY)
                            .text_style(egui::TextStyle::Small),
                    );
                    if ui.button("ⓘ").clicked() {
                        self.show_about = !self.show_about;
                    }
                });

                if self.git.is_open() {
                    ui.separator();
                    let project_name = Path::new(&self.repo_path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| self.repo_path.clone());
                    if ui.button("⏰ History").clicked() {
                        self.current_tab = Tab::Log;
                    }
                    ui.label(
                        egui::RichText::new(project_name)
                            .color(egui::Color32::from_rgb(100, 150, 255))
                            .strong(),
                    );
                    ui.separator();

                    let branch = self.git.current_branch().unwrap_or_default();
                    ui.label(format!("🔀 {}", branch));

                    let status_count = self.status_entries.len();
                    if status_count > 0 {
                        ui.label(
                            egui::RichText::new(format!("{} changes", status_count))
                                .color(egui::Color32::YELLOW),
                        );
                    }

                    if !self.remote_list.is_empty() {
                        let remote = &self.remote_list[0];
                        ui.label(format!("🌐 {}", remote.name));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Show update indicator if an update is available
                        {
                            let state = self.update_state.lock().unwrap();
                            if matches!(*state, UpdateState::UpdateAvailable { .. }) {
                                ui.label(
                                    egui::RichText::new("⬆ Update")
                                        .color(egui::Color32::YELLOW),
                                );
                            }
                        }
                        // Disable refresh button while busy
                        if ui.add_enabled(!self.is_busy(), egui::Button::new("🔄")).clicked() {
                            self.refresh_all();
                        }
                        if ui.button("ⓘ").clicked() {
                            self.show_about = !self.show_about;
                        }
                    });
                }
            });
        });

        // --- Bottom Bar (messages + status) ---
        if self.git.is_open() {
            egui::TopBottomPanel::bottom("bottom_bar")
                .resizable(false)
                .min_height(24.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        // Loading indicator when operations are in progress
                        if self.is_busy() {
                            ui.label(
                                egui::RichText::new(format!("⏳ {}...", self.current_operation()))
                                    .color(egui::Color32::YELLOW)
                                    .size(13.0),
                            );
                        }

                        let has_message =
                            !self.error_message.is_empty() || !self.success_message.is_empty();
                        if has_message && !self.is_busy() {
                            if !self.error_message.is_empty() {
                                if self.status_expanded {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&self.error_message)
                                                .color(egui::Color32::RED),
                                        )
                                        .wrap(),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new(&self.error_message)
                                            .color(egui::Color32::RED),
                                    );
                                }
                            }
                            if !self.success_message.is_empty() {
                                if self.status_expanded {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&self.success_message)
                                                .color(egui::Color32::GREEN),
                                        )
                                        .wrap(),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new(&self.success_message)
                                            .color(egui::Color32::GREEN),
                                    );
                                }
                            }
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if has_message && !self.is_busy() {
                                    let expand_label = if self.status_expanded {
                                        "▼"
                                    } else {
                                        "▲"
                                    };
                                    if ui.button(expand_label).clicked() {
                                        self.status_expanded = !self.status_expanded;
                                    }
                                    if ui.button("x").clicked() {
                                        self.error_message.clear();
                                        self.success_message.clear();
                                    }
                                }
                                ui.separator();
                                ui.label("Font:");
                                ui.add(
                                    egui::Slider::new(&mut self.font_size, Self::MIN_FONT_SIZE..=Self::MAX_FONT_SIZE)
                                        .show_value(false),
                                );
                                let elapsed = self.last_refresh.elapsed().as_secs();
                                ui.label(format!("Updated {}s ago", elapsed));
                            },
                        );
                    });
                });
            }

        // --- Central Panel ---
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.git.is_open() {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("Git Manager");
                    ui.label("Open a Git repository to get started.");
                    ui.add_space(20.0);
                    let open_btn = egui::Button::new("📂 Open Repository");
                    if ui.add_enabled(!self.is_busy(), open_btn).clicked() {
                        let path = crate::native_file_dialog();
                        if let Some(p) = path {
                            self.open_repo(&p);
                        }
                    }
                    ui.add_space(10.0);
                    ui.label("Or drag & drop a folder");
                    if ui.button("Clone Repository...").clicked() {
                        self.current_tab = Tab::Remotes;
                    }

                    // Recent repositories section
                    if !self.recent_repos.is_empty() {
                        ui.add_space(30.0);
                        ui.separator();
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("📁 Recent Repositories")
                                .heading(),
                        );
                        ui.add_space(5.0);

                        let mut to_delete: Option<usize> = None;
                        let entries = self.recent_repos.entries().to_vec();
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for (i, entry) in entries.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.set_min_width(400.0);
                                        let repo_name = format!("📂 {}", entry.name);
                                        if ui
                                            .selectable_label(false, egui::RichText::new(&repo_name).size(14.0))
                                            .clicked()
                                        {
                                            self.open_repo(&entry.path);
                                        }
                                        ui.label(
                                            egui::RichText::new(&entry.path)
                                                .size(10.0)
                                                .color(egui::Color32::GRAY),
                                        );
                                        if ui.button("🗑 Delete").clicked() {
                                            to_delete = Some(i);
                                        }
                                    });
                                }
                            });
                        if let Some(idx) = to_delete {
                            self.recent_repos.remove(idx);
                        }
                    }
                });
                return;
            }

            // Tab bar
            ui.horizontal(|ui| {
                let tabs = [
                    (Tab::Status, "📊 Status"),
                    (Tab::Branches, "🔀 Branches"),
                    (Tab::Worktrees, "📂 Worktrees"),
                    (Tab::Log, "⏰ Log"),
                    (Tab::Stash, "📦 Stash"),
                    (Tab::Remotes, "🌐 Remotes"),
                ];

                for (tab, label) in &tabs {
                    let selected = self.current_tab == *tab;
                    let btn = egui::Button::new(*label)
                        .fill(if selected {
                            ui.style().visuals.selection.bg_fill
                        } else {
                            egui::Color32::TRANSPARENT
                        });
                    if ui.add(btn).clicked() {
                        self.current_tab = tab.clone();
                    }
                }
            });

            ui.separator();

            // Render the active tab panel
            match self.current_tab {
                Tab::Status => crate::ui::status_panel::show(self, ui, ctx),
                Tab::Branches => crate::ui::branch_panel::show(self, ui, ctx),
                Tab::Worktrees => crate::ui::worktree_panel::show(self, ui, ctx),
                Tab::Log => crate::ui::log_panel::show(self, ui, ctx),
                Tab::Stash => crate::ui::stash_panel::show(self, ui, ctx),
                Tab::Remotes => crate::ui::remote_panel::show(self, ui, ctx),
            }
        });

        // About window
        if self.show_about {
            egui::Window::new("About Git Manager")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Git Manager");
                        ui.add_space(4.0);
                        ui.label(format!("Version: {}", crate::version_info::VERSION));
                        ui.label(format!("Commit: {}", crate::version_info::GIT_HASH));
                        ui.label(format!("Tag: {}", crate::version_info::GIT_DESCRIBE));
                        ui.label(format!("Build: {}", crate::version_info::BUILD_DATE));
                        ui.add_space(8.0);
                        ui.hyperlink("https://github.com/JohnXu22786/GitManager");
                        ui.add_space(8.0);
                        ui.label("A dedicated Git branch & worktree manager.");
                        ui.add_space(4.0);
                        ui.label("Built with Rust + egui + libgit2");
                        ui.add_space(12.0);

                        // Check for Updates button
                        {
                            let state = self.update_state.lock().unwrap().clone();
                            match state {
                                UpdateState::Idle | UpdateState::UpToDate => {
                                    if ui.button("Check for Updates").clicked() {
                                        self.trigger_update_check();
                                    }
                                    if state == UpdateState::UpToDate {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new("✓ You're up to date!").color(egui::Color32::GREEN));
                                    }
                                }
                                UpdateState::Checking => {
                                    ui.add(egui::Spinner::new());
                                    ui.label("Checking for updates...");
                                }
                                UpdateState::UpdateAvailable { latest_version, download_url } => {
                                    ui.colored_label(egui::Color32::YELLOW, format!("Update available: {}!", latest_version));
                                    if ui.button("Download").clicked() {
                                        let _ = open::that(&download_url);
                                    }
                                }
                                UpdateState::Error(ref msg) => {
                                    ui.colored_label(egui::Color32::RED, msg.as_str());
                                    if ui.button("Retry").clicked() {
                                        self.trigger_update_check();
                                    }
                                }
                            }
                        }

                        ui.add_space(8.0);
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }

        // Auto-update notification dialog
        if self.show_update_dialog {
            let state = self.update_state.lock().unwrap().clone();
            if let UpdateState::UpdateAvailable { ref latest_version, ref download_url } = state {
                egui::Window::new("Update Available")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.heading("🚀 Update Available!");
                            ui.add_space(8.0);
                            ui.label(format!(
                                "Version {} is now available (you have {}).",
                                latest_version,
                                env!("CARGO_PKG_VERSION"),
                            ));
                            ui.add_space(8.0);
                            ui.label("Visit the GitHub releases page to download the latest version.");
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.button("Download").clicked() {
                                    let _ = open::that(download_url);
                                    self.show_update_dialog = false;
                                }
                                if ui.button("Remind Later").clicked() {
                                    self.show_update_dialog = false;
                                }
                            });
                        });
                    });
            } else {
                // State changed while dialog was open
                self.show_update_dialog = false;
            }
        }

        // Keep repainting while operations are in progress
        if self.is_busy() {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new_defaults() {
        let app = App::new();
        assert_eq!(app.font_size, 14.0);
        assert!(!app.show_about);
        assert!(!app.git.is_open());
        assert_eq!(app.current_tab, Tab::Status);
    }

    #[test]
    fn test_font_size_default_in_range() {
        let app = App::new();
        assert!(app.font_size >= App::MIN_FONT_SIZE);
        assert!(app.font_size <= App::MAX_FONT_SIZE);
    }

    #[test]
    fn test_status_color_by_type_known() {
        assert_eq!(App::status_color_by_type('M'), egui::Color32::GREEN);
        assert_eq!(App::status_color_by_type('A'), egui::Color32::GREEN);
        assert_eq!(App::status_color_by_type('R'), egui::Color32::GREEN);
        assert_eq!(App::status_color_by_type('D'), egui::Color32::RED);
        assert_eq!(App::status_color_by_type('?'), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('!'), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('U'), egui::Color32::YELLOW);
    }

    #[test]
    fn test_status_color_by_type_unknown() {
        assert_eq!(App::status_color_by_type('X'), egui::Color32::GRAY);
    }

    #[test]
    fn test_tab_partial_eq() {
        assert_eq!(Tab::Status, Tab::Status);
        assert_eq!(Tab::Log, Tab::Log);
        assert_ne!(Tab::Status, Tab::Branches);
    }

    #[test]
    fn test_tab_clone() {
        assert_eq!(Tab::Worktrees.clone(), Tab::Worktrees);
    }
}
