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
    /// Real-time progress text updated by the background thread (e.g. "Receiving objects: 45%").
    progress: Arc<Mutex<String>>,
    /// Tracks the last time the progress text changed (watchdog timer).
    last_progress_update: Instant,
    /// The last progress value we read (to detect changes).
    last_seen_progress: String,
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
    /// Accumulated real-time log of the latest operation (progress + final result).
    /// Used in the expandable bottom panel so users see detailed CLI-like output.
    pub last_operation_log: String,

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
    pub update_state: Arc<Mutex<UpdateState>>,
    pub show_update_dialog: bool,
    pub auto_check_done: bool,
    /// Set to true when the user clicks "Remind Later" to prevent the dialog from reopening.
    pub update_dialog_dismissed: bool,
    /// Download progress from 0.0 to 1.0 for the current download.
    pub download_progress: f32,
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
    /// When true, shows a popup window with the full status message.
    pub show_message_popup: bool,
    /// Excel-style resizable column widths for tables.
    pub column_widths: crate::ui::ColumnWidthStore,
}

impl App {
    const FONT_SIZE: f32 = 14.0;

    pub fn new() -> Self {
        Self {
            git: GitRepo::new(),
            current_tab: Tab::Status,
            repo_path: String::new(),
            error_message: String::new(),
            success_message: String::new(),
            last_operation_log: String::new(),

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

            update_state: Arc::new(Mutex::new(UpdateState::Idle)),
            show_update_dialog: false,
            auto_check_done: false,
            update_dialog_dismissed: false,
            download_progress: 0.0,
            pending_ops: Vec::new(),
            pending_errors: Vec::new(),
            pending_successes: Vec::new(),
            needs_refresh: false,
            recent_repos: RecentRepos::load(),
            status_expanded: false,
            show_message_popup: false,
            column_widths: crate::ui::init_column_widths(),
        }
    }

    pub fn trigger_update_check(&mut self) {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let state = self.update_state.clone();
        *state.lock().unwrap() = UpdateState::Checking;
        self.update_dialog_dismissed = false;
        self.show_update_dialog = false;

        std::thread::spawn(move || {
            let result = updater::check_for_update(&current_version);
            *state.lock().unwrap() = result;
        });
    }

    /// Start downloading the update asset in a background thread.
    /// Updates `update_state` with progress as the download proceeds.
    pub fn trigger_download(&mut self, url: String, file_name: String) {
        let state = self.update_state.clone();
        let progress = Arc::new(Mutex::new(0.0f32));
        let prog = progress.clone();
        let state_for_progress = state.clone();
        *state.lock().unwrap() = UpdateState::Downloading {
            progress: 0.0,
            file_name: file_name.clone(),
        };
        self.download_progress = 0.0;

        std::thread::spawn(move || {
            // Save to Downloads folder
            let dest_dir = updater::get_default_download_dir();
            let dest_path = std::path::Path::new(&dest_dir).join(&file_name);

            // Update progress in real-time from the background thread
            let prog_clone = prog.clone();
            let _prog_update_handle = std::thread::spawn(move || {
                loop {
                    let p = *prog_clone.lock().unwrap();
                    let current_state = state_for_progress.lock().unwrap().clone();
                    let is_downloading = matches!(current_state, UpdateState::Downloading { .. });
                    if !is_downloading {
                        break;
                    }
                    *state_for_progress.lock().unwrap() = UpdateState::Downloading {
                        progress: p,
                        file_name: file_name.clone(),
                    };
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });

            let result = updater::download_file_with_progress(&url, &dest_path, prog);

            match result {
                Ok(()) => {
                    let path_str = dest_path.to_string_lossy().to_string();
                    *state.lock().unwrap() = UpdateState::Downloaded {
                        file_path: path_str,
                    };
                }
                Err(e) => {
                    *state.lock().unwrap() = UpdateState::Error(e);
                }
            }
        });
    }

    /// Extract the binary from the downloaded archive, create a self-update
    /// script, launch it, and exit the current process to complete the update.
    pub fn install_and_restart(&mut self) {
        let current_state = self.update_state.lock().unwrap().clone();
        let archive_path = match &current_state {
            UpdateState::Downloaded { file_path } => file_path.clone(),
            _ => return,
        };

        let archive_path = std::path::Path::new(&archive_path).to_path_buf();

        // Extract binary from archive
        let new_binary = match updater::extract_binary_from_archive(&archive_path) {
            Ok(p) => p,
            Err(e) => {
                self.error_message = format!("Failed to extract update: {}", e);
                return;
            }
        };

        // Get current executable path
        let current_binary = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                self.error_message = format!("Failed to get current exe path: {}", e);
                return;
            }
        };

        // Create self-update script
        let script_path = match updater::create_self_update_script(&new_binary, &current_binary) {
            Ok(p) => p,
            Err(e) => {
                self.error_message = format!("Failed to create update script: {}", e);
                return;
            }
        };

        // Launch the script (detached from parent process)
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", script_path.to_str().unwrap_or("")])
                .spawn();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("sh")
                .arg(script_path.to_str().unwrap_or(""))
                .spawn();
        }

        // Exit the current process immediately
        std::process::exit(0);
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

    /// Returns the description of the current/last operation, including real-time progress.
    pub fn current_operation(&self) -> String {
        self.pending_ops.first()
            .map(|op| {
                let progress = op.progress.lock().unwrap().clone();
                if progress.is_empty() {
                    op.description.clone()
                } else {
                    format!("{}: {}", op.description, progress)
                }
            })
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
        let progress = Arc::new(Mutex::new(String::new()));
        let op_progress = progress.clone();

        std::thread::spawn(move || {
            let result = execute_operation(&repo_path, op, op_progress);
            let _ = tx.send(result);
        });

        self.pending_ops.push(PendingOp {
            description: desc,
            receiver: rx,
            started_at: Instant::now(),
            progress,
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
        });

        // Initialize the operation log with description
        self.last_operation_log = format!("▶ Operation: {}\n", description);
        // Ensure log has a trailing placeholder so progress can accumulate
        self.last_operation_log += "  (waiting for progress...)\n";

        ctx.request_repaint();
    }

    /// Process completed background operations.
    /// Must be called at the start of each `update()` frame.
    /// Uses a progress-watchdog timeout: keeps waiting while the progress string keeps
    /// changing (operation is still alive). Times out when progress stops for too long,
    /// or when no progress was ever received beyond a reasonable limit.
    pub fn process_pending_ops(&mut self, ctx: &egui::Context) {
        let mut i = 0;
        while i < self.pending_ops.len() {
            // --- Watchdog timeout: check if the operation is still making progress ---
            let (description, started_at, current_progress, last_seen_progress, last_progress_update) = {
                let op = &self.pending_ops[i];
                let description = op.description.clone();
                let started_at = op.started_at;
                let current_progress = op.progress.lock().unwrap().clone();
                let last_seen_progress = op.last_seen_progress.clone();
                let last_progress_update = op.last_progress_update;
                (description, started_at, current_progress, last_seen_progress, last_progress_update)
            };

            // If progress text changed, reset the watchdog timer and accumulate to log
            if current_progress != last_seen_progress {
                if let Some(mut_op) = self.pending_ops.get_mut(i) {
                    mut_op.last_progress_update = Instant::now();
                    mut_op.last_seen_progress = current_progress.clone();
                }
                if !current_progress.is_empty() {
                    self.last_operation_log += &format!("  {}\n", current_progress);
                }
            } else {
                let stall_secs = last_progress_update.elapsed().as_secs();
                if current_progress.is_empty() {
                    // No progress ever received: give 60 seconds total
                    if started_at.elapsed().as_secs() > 60 {
                        let msg = format!(
                            "Operation '{}' timed out (no progress in 60s)",
                            description
                        );
                        self.pending_errors.push(msg.clone());
                        self.last_operation_log += &format!("  ✗ {}\n", msg);
                        self.pending_ops.swap_remove(i);
                        continue;
                    }
                } else {
                    // Progress was received but stopped: 30 second stall threshold
                    if stall_secs > 30 {
                        let msg = format!(
                            "Operation '{}' timed out (stalled {}s)\nLast: {}",
                            description, stall_secs, current_progress
                        );
                        self.pending_errors.push(msg.clone());
                        self.last_operation_log += &format!("  ✗ {}\n", msg);
                        self.pending_ops.swap_remove(i);
                        continue;
                    }
                }
            }

            let op = &self.pending_ops[i];
            match op.receiver.try_recv() {
                Ok(result) => {
                    let op = self.pending_ops.swap_remove(i);
                    // Append final progress to log before handling result
                    let final_progress = current_progress.clone();
                    if !final_progress.is_empty() {
                        self.last_operation_log += &format!("  {}\n", final_progress);
                    }
                    self.handle_op_result(op.description, result);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1; // Still pending
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let last_prog = op.progress.lock().unwrap().clone();
                    let fail_msg = if last_prog.is_empty() {
                        format!("Operation '{}' failed unexpectedly", op.description)
                    } else {
                        format!("Operation '{}' failed unexpectedly\nLast progress: {}", op.description, last_prog)
                    };
                    self.pending_errors.push(fail_msg.clone());
                    self.last_operation_log += &format!("  ✗ {}\n", fail_msg);
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
                self.last_operation_log += &format!("  ✓ {}\n", msg);
                self.pending_successes.push(msg);
                // Auto-refresh after mutation operations
                self.needs_refresh = true;
            }
            OpResult::Error(e) => {
                let err_msg = format!("{}: {}", description, e);
                self.last_operation_log += &format!("  ✗ {}\n", err_msg);
                self.pending_errors.push(err_msg);
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
    /// New errors OVERWRITE old ones (requirement: "no persistent display, new info overwrites").
    pub fn flush_messages(&mut self) {
        if !self.pending_successes.is_empty() {
            self.success_message = self.pending_successes.join(" | ");
            self.pending_successes.clear();
        }
        if !self.pending_errors.is_empty() {
            let msg = self.pending_errors.join(" | ");
            self.pending_errors.clear();
            // Overwrite: new errors replace old ones (do not append)
            self.error_message = msg;
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

    /// Returns the project folder name extracted from the repo path.
    /// e.g. "/home/user/projects/my-repo" → "my-repo"
    pub fn repo_name(&self) -> String {
        if self.repo_path.is_empty() {
            return String::new();
        }
        std::path::Path::new(&self.repo_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.repo_path.clone())
    }

    /// Format elapsed seconds into a human-readable string.
    /// <60s → "Just updated", <3600s → "Updated Xm ago", ≥3600s → "Updated Xh ago"
    pub fn format_elapsed(elapsed_secs: u64) -> String {
        if elapsed_secs < 60 {
            "Just updated".to_string()
        } else if elapsed_secs < 3600 {
            format!("Updated {}m ago", elapsed_secs / 60)
        } else {
            format!("Updated {}h ago", elapsed_secs / 3600)
        }
    }

    /// Return an adaptive green color suitable for both dark and light mode.
    pub fn adaptive_green(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(80, 220, 80)   // Bright green on dark bg
        } else {
            egui::Color32::from_rgb(0, 120, 0)       // Dark green on light bg
        }
    }

    /// Return an adaptive yellow/amber color suitable for both dark and light mode.
    pub fn adaptive_yellow(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(220, 200, 50)    // Bright yellow on dark bg
        } else {
            egui::Color32::from_rgb(180, 130, 0)      // Dark amber on light bg
        }
    }

    /// Return an adaptive red color suitable for both dark and light mode.
    pub fn adaptive_red(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(240, 80, 80)      // Bright red on dark bg
        } else {
            egui::Color32::from_rgb(180, 30, 30)      // Dark red on light bg
        }
    }

    /// Return an adaptive gold/amber color suitable for both dark and light mode.
    pub fn adaptive_gold(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(255, 200, 50)     // Bright gold on dark bg
        } else {
            egui::Color32::from_rgb(180, 140, 0)      // Dark gold on light bg
        }
    }

    pub fn status_color_by_type(s: char, dark: bool) -> egui::Color32 {
        match s {
            'M' | 'A' | 'R' => Self::adaptive_green(dark),
            'D' => Self::adaptive_red(dark),
            'U' => Self::adaptive_yellow(dark),  // Conflicted
            _ => egui::Color32::GRAY,            // '?' untracked, '!' ignored, or unknown
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
                UpdateState::UpdateAvailable { latest_version, download_url: _, assets: _ } => {
                    if !self.update_dialog_dismissed && !self.show_update_dialog {
                        self.show_update_dialog = true;
                    }
                    let _ = latest_version;
                }
                UpdateState::Checking => {
                    ctx.request_repaint();
                }
                UpdateState::Downloading { progress, file_name: _ } => {
                    self.download_progress = *progress;
                    ctx.request_repaint();
                }
                UpdateState::Downloaded { file_path: _ } => {
                    self.download_progress = 1.0;
                }
                _ => {}
            }
        }
        self.process_pending_ops(ctx);
        self.flush_messages();

        let dark = ctx.style().visuals.dark_mode;
        if dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Apply font size via text styles
        let fs = Self::FONT_SIZE;
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
                if crate::ui::add_enabled_ellipsis(ui, !self.is_busy(), "📂").clicked() {
                    let path = crate::native_file_dialog();
                    if let Some(p) = path {
                        self.open_repo(&p);
                    }
                }

                // Recent repos dropdown
                egui::menu::menu_button(ui, "🕒", |ui| {
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
                                if crate::ui::ellipsis_button(ui, "🗑").clicked() {
                                    to_delete = Some(i);
                                }
                            });
                        }
                        if let Some(idx) = to_delete {
                            self.recent_repos.remove(idx);
                        }
                    }
                });

                if self.git.is_open() {
                    ui.separator();
                    let project_name = self.repo_name();

                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &project_name,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(14.0),
                            color: egui::Color32::from_rgb(100, 150, 255),
                            ..Default::default()
                        },
                    );
                    job.append(
                        "  ",
                        0.0,
                        egui::TextFormat::default(),
                    );
                    job.append(
                        &self.repo_path,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(11.0),
                            color: egui::Color32::GRAY,
                            ..Default::default()
                        },
                    );
                    ui.add(
                        egui::Label::new(job).truncate(),
                    )
                    .on_hover_text(format!("{}\n{}", project_name, self.repo_path));

                    // Right-side elements anchored to right edge: version, about, update indicator
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Show update indicator if an update is available
                        {
                            let state = self.update_state.lock().unwrap();
                            if matches!(*state, UpdateState::UpdateAvailable { .. }) {
                                ui.label(
                                    egui::RichText::new("⬆ Update")
                                        .color(App::adaptive_yellow(dark)),
                                );
                            }
                        }
                        // About button
                        if ui.button("ⓘ").clicked() {
                            self.show_about = !self.show_about;
                        }
                        // Version label (truncatable so it doesn't push buttons off-screen)
                        let version_text = format!("v{}", crate::version_info::VERSION);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&version_text)
                                    .color(egui::Color32::GRAY)
                                    .text_style(egui::TextStyle::Small),
                            )
                            .truncate(),
                        )
                        .on_hover_text(version_text);
                    });

                    ui.separator();
                    // Make repo path label truncatable when window is too narrow
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.repo_path)
                                .color(egui::Color32::from_rgb(100, 150, 255)),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&self.repo_path);
                    ui.separator();

                    let branch = self.git.current_branch().unwrap_or_default();
                    let branch_text = format!("🔀 {}", branch);
                    ui.add(
                        egui::Label::new(&branch_text)
                            .truncate(),
                    )
                    .on_hover_text(&branch_text);

                    let status_count = self.status_entries.len();
                    if status_count > 0 {
                        let status_text = format!("{} changes", status_count);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&status_text)
                                    .color(egui::Color32::YELLOW),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&status_text);
                    }

                    if !self.remote_list.is_empty() {
                        let remote = &self.remote_list[0];
                        let remote_text = format!("🌐 {}", remote.name);
                        ui.add(
                            egui::Label::new(&remote_text)
                                .truncate(),
                        )
                        .on_hover_text(&remote_text);
                    }
                } else {
                    // No repo open: show version + about on the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("ⓘ").clicked() {
                            self.show_about = !self.show_about;
                        }
                        let version_text = format!("v{}", crate::version_info::VERSION);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&version_text)
                                    .color(egui::Color32::GRAY)
                                    .text_style(egui::TextStyle::Small),
                            )
                            .truncate(),
                        )
                        .on_hover_text(version_text);
                    });
                }
            });
        });

        // --- Bottom Bar (messages + status) ---
        if self.git.is_open() {
            let bottom_height = if self.status_expanded { 160.0 } else { 24.0 };
            egui::TopBottomPanel::bottom("bottom_bar")
                .resizable(false)
                .min_height(bottom_height)
                .show(ctx, |ui| {
                    let dark = ctx.style().visuals.dark_mode;
                    ui.horizontal(|ui| {
                        // Loading indicator when operations are in progress
                        if self.is_busy() {
                            let op_text = self.current_operation();
                            ui.label(
                                egui::RichText::new(format!("⏳ {}...", op_text))
                                    .color(App::adaptive_yellow(dark))
                                    .size(13.0),
                            );
                        }

                        // Expand button always visible
                        let expand_label = if self.status_expanded { "▲" } else { "▼" };
                        if ui.button(expand_label).clicked() {
                            self.status_expanded = !self.status_expanded;
                        }

                        // Right side: refresh timestamp + messages (no close button, no auto-dismiss)
                        let has_message =
                            !self.error_message.is_empty() || !self.success_message.is_empty();
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // No close button — messages persist, latest only
                                ui.separator();
                                let elapsed = self.last_refresh.elapsed().as_secs();
                                let elapsed_text = App::format_elapsed(elapsed);
                                ui.add(
                                    egui::Label::new(&elapsed_text)
                                        .truncate(),
                                )
                                .on_hover_text(elapsed_text);

                                if has_message {
                                    if !self.error_message.is_empty() {
                                        let msg_text = self.error_message.clone();
                                        let resp = ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&msg_text)
                                                    .color(App::adaptive_red(dark)),
                                            )
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_text(msg_text);
                                        if resp.clicked() {
                                            self.show_message_popup = true;
                                        }
                                    }
                                    if !self.success_message.is_empty() {
                                        let msg_text = self.success_message.clone();
                                        let resp = ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&msg_text)
                                                    .color(App::adaptive_green(dark)),
                                            )
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_text(msg_text);
                                        if resp.clicked() {
                                            self.show_message_popup = true;
                                        }
                                    }
                                }
                            },
                        );
                    });

                    // Expandable command output area (visible whenever expanded)
                    if self.status_expanded {
                        ui.separator();
                        let output_text = if self.is_busy() {
                            self.current_operation()
                        } else {
                            self.last_operation_log.clone()
                        };
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(output_text)
                                        .monospace()
                                        .size(12.0),
                                );
                                ui.allocate_space(ui.available_size());
                            });
                    }
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
                    if crate::ui::add_enabled_ellipsis(ui, !self.is_busy(), "📂 Open Repository").clicked() {
                        let path = crate::native_file_dialog();
                        if let Some(p) = path {
                            self.open_repo(&p);
                        }
                    }
                    ui.add_space(10.0);
                    ui.label("Or drag & drop a folder");
                    if crate::ui::ellipsis_button(ui, "Clone Repository...").clicked() {
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
                                        if crate::ui::ellipsis_button(ui, "🗑 Delete").clicked() {
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

            // Tab bar — wrapped in horizontal ScrollArea so tabs don't overflow when window is narrow
            egui::ScrollArea::horizontal()
                .id_salt("tab_bar_scroll")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let tabs = [
                            (Tab::Status, "📊 Status"),
                            (Tab::Branches, "🔀 Branches"),
                            (Tab::Worktrees, "📂 Worktrees"),
                            (Tab::Log, "📋 Log"),
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
                                    if crate::ui::ellipsis_button(ui, "Check for Updates").clicked() {
                                        self.trigger_update_check();
                                    }
                                    if state == UpdateState::UpToDate {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new("✓ You're up to date!").color(App::adaptive_green(dark)));
                                    }
                                }
                                UpdateState::Checking => {
                                    ui.add(egui::Spinner::new());
                                    ui.label("Checking for updates...");
                                }
                                UpdateState::UpdateAvailable { latest_version, download_url, ref assets } => {
                                    ui.colored_label(App::adaptive_yellow(dark), format!("Update available: {}!", latest_version));
                                    // Try auto-download if matching asset is available
                                    if let Some((asset_url, file_name)) = updater::find_asset_for_current_platform(assets) {
                                        if crate::ui::ellipsis_button(ui, "Download & Install").clicked() {
                                            self.trigger_download(asset_url, file_name);
                                        }
                                    } else {
                                        // Fallback: open browser
                                        if crate::ui::ellipsis_button(ui, "Download (Browser)").clicked() {
                                            let _ = open::that(&download_url);
                                        }
                                    }
                                }
                                UpdateState::Downloading { progress, file_name } => {
                                    ui.colored_label(App::adaptive_yellow(dark), format!("Downloading: {} ({:.0}%)", file_name, progress * 100.0));
                                    ui.add(
                                        egui::ProgressBar::new(progress)
                                            .show_percentage()
                                            .animate(true),
                                    );
                                }
                UpdateState::Downloaded { file_path } => {
                                    ui.colored_label(App::adaptive_green(dark), "✓ Download complete!");
                                    ui.label(egui::RichText::new(&file_path).size(10.0).color(egui::Color32::GRAY));
                                }
                                UpdateState::Error(ref msg) => {
                                    ui.colored_label(App::adaptive_red(dark), msg.as_str());
                                    if crate::ui::ellipsis_button(ui, "Retry").clicked() {
                                        self.trigger_update_check();
                                    }
                                }
                            }
                        }

                        ui.add_space(8.0);
                        if crate::ui::ellipsis_button(ui, "Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }

        // Auto-update notification dialog
        // Note: we intentionally keep show_update_dialog = true during download
        // so the dialog shows the Downloading → Downloaded state transitions.
        // The dialog only closes when the user explicitly dismisses it.
        if self.show_update_dialog {
            let current_state = self.update_state.lock().unwrap().clone();
            match &current_state {
                UpdateState::UpdateAvailable { latest_version, download_url, assets } => {
                    // If download is in progress (triggered from About window), switch to download view
                    if matches!(current_state, UpdateState::Downloading { .. }) {
                        // Let the next match arm handle it
                    } else {
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
                                    ui.label("An automatic download is available below.");
                                    ui.add_space(12.0);
                                    ui.horizontal(|ui| {
                                        // Try auto-download first
                                        if let Some((asset_url, file_name)) = updater::find_asset_for_current_platform(assets) {
                                            if crate::ui::ellipsis_button(ui, "Auto Download").clicked() {
                                                self.trigger_download(asset_url, file_name);
                                                // Keep dialog open to show progress
                                            }
                                        }
                                        // Fallback: open browser
                                        if crate::ui::ellipsis_button(ui, "Open in Browser").clicked() {
                                            let _ = open::that(download_url);
                                            self.show_update_dialog = false;
                                        }
                                        if crate::ui::ellipsis_button(ui, "Remind Later").clicked() {
                                            self.show_update_dialog = false;
                                            self.update_dialog_dismissed = true;
                                        }
                                    });
                                });
                            });
                    }
                }
                UpdateState::Downloading { progress, file_name } => {
                    egui::Window::new("Downloading Update")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("⬇ Downloading Update");
                                ui.add_space(8.0);
                                ui.label(format!("Downloading: {}...", file_name));
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(*progress)
                                        .show_percentage()
                                        .animate(true),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!("{:.1}%", *progress * 100.0))
                                        .color(App::adaptive_yellow(ctx.style().visuals.dark_mode)),
                                );
                            });
                        });
                }
                UpdateState::Downloaded { file_path } => {
                    egui::Window::new("Download Complete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("✅ Download Complete!");
                                ui.add_space(8.0);
                                ui.label("The update has been downloaded successfully.");
                                ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(file_path.clone())
                                            .size(10.0)
                                            .color(egui::Color32::GRAY),
                                    ).on_hover_text(file_path.clone());
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if crate::ui::ellipsis_button(ui, "Install & Restart").clicked() {
                                        self.install_and_restart();
                                    }
                                    if crate::ui::ellipsis_button(ui, "Dismiss").clicked() {
                                        *self.update_state.lock().unwrap() = UpdateState::Idle;
                                    }
                                });
                            });
                        });
                }
                _ => {
                    // State changed while dialog was open (or no longer relevant)
                    self.show_update_dialog = false;
                }
            }
        }

        // Keep repainting while operations are in progress
        if self.is_busy() {
            ctx.request_repaint();
        }

        // --- Message popup window (clicking a truncated status message opens this) ---
        if self.show_message_popup {
            let has_error = !self.error_message.is_empty();
            let has_success = !self.success_message.is_empty();
            let popup_title = if has_error {
                "Error Details"
            } else if has_success {
                "Success Details"
            } else {
                "Message"
            };
            let popup_content = if has_error {
                self.error_message.clone()
            } else if has_success {
                self.success_message.clone()
            } else {
                String::new()
            };

            egui::Window::new(popup_title)
                .resizable(true)
                .default_size([500.0, 200.0])
                .min_width(250.0)
                .min_height(100.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut self.show_message_popup)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let color = if has_error {
                                App::adaptive_red(dark)
                            } else {
                                App::adaptive_green(dark)
                            };
                            ui.label(
                                egui::RichText::new(&popup_content)
                                    .color(color)
                                    .size(14.0),
                            );
                        });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Existing tests ---

    #[test]
    fn test_font_size_constant_is_14() {
        assert_eq!(App::FONT_SIZE, 14.0);
    }

    #[test]
    fn test_app_new_defaults() {
        let app = App::new();
        assert!(!app.show_about);
        assert!(!app.git.is_open());
        assert_eq!(app.current_tab, Tab::Status);
    }

    #[test]
    fn test_status_color_by_type_known_dark() {
        assert_eq!(App::status_color_by_type('M', true), egui::Color32::from_rgb(80, 220, 80));
        assert_eq!(App::status_color_by_type('A', true), egui::Color32::from_rgb(80, 220, 80));
        assert_eq!(App::status_color_by_type('R', true), egui::Color32::from_rgb(80, 220, 80));
        assert_eq!(App::status_color_by_type('D', true), egui::Color32::from_rgb(240, 80, 80));
        assert_eq!(App::status_color_by_type('U', true), egui::Color32::from_rgb(220, 200, 50));
    }

    #[test]
    fn test_status_color_by_type_known_light() {
        assert_eq!(App::status_color_by_type('M', false), egui::Color32::from_rgb(0, 120, 0));
        assert_eq!(App::status_color_by_type('A', false), egui::Color32::from_rgb(0, 120, 0));
        assert_eq!(App::status_color_by_type('R', false), egui::Color32::from_rgb(0, 120, 0));
        assert_eq!(App::status_color_by_type('D', false), egui::Color32::from_rgb(180, 30, 30));
        assert_eq!(App::status_color_by_type('U', false), egui::Color32::from_rgb(180, 130, 0));
    }

    #[test]
    fn test_status_color_by_type_unknown() {
        assert_eq!(App::status_color_by_type('X', true), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('X', false), egui::Color32::GRAY);
    }

    #[test]
    fn test_status_color_gray_untracked() {
        assert_eq!(App::status_color_by_type('?', true), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('!', true), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('?', false), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('!', false), egui::Color32::GRAY);
    }

    #[test]
    fn test_adaptive_color_dark_vs_light_different() {
        // Green should be different in dark vs light mode
        assert_ne!(
            App::status_color_by_type('M', true),
            App::status_color_by_type('M', false)
        );
        // Yellow/conflict should be different
        assert_ne!(
            App::status_color_by_type('U', true),
            App::status_color_by_type('U', false)
        );
        // Red should be different
        assert_ne!(
            App::status_color_by_type('D', true),
            App::status_color_by_type('D', false)
        );
        // Gray should stay the same
        assert_eq!(
            App::status_color_by_type('?', true),
            App::status_color_by_type('?', false)
        );
        assert_eq!(
            App::status_color_by_type('!', true),
            App::status_color_by_type('!', false)
        );
    }

    #[test]
    fn test_format_elapsed_just_updated() {
        assert_eq!(App::format_elapsed(0), "Just updated");
        assert_eq!(App::format_elapsed(1), "Just updated");
        assert_eq!(App::format_elapsed(30), "Just updated");
        assert_eq!(App::format_elapsed(59), "Just updated");
    }

    #[test]
    fn test_format_elapsed_minutes() {
        assert_eq!(App::format_elapsed(60), "Updated 1m ago");
        assert_eq!(App::format_elapsed(120), "Updated 2m ago");
        assert_eq!(App::format_elapsed(3540), "Updated 59m ago");
    }

    #[test]
    fn test_format_elapsed_hours() {
        assert_eq!(App::format_elapsed(3600), "Updated 1h ago");
        assert_eq!(App::format_elapsed(7200), "Updated 2h ago");
        assert_eq!(App::format_elapsed(86400), "Updated 24h ago");
    }

    #[test]
    fn test_format_elapsed_boundaries() {
        // 59 seconds → "Just updated"
        assert_eq!(App::format_elapsed(59), "Just updated");
        // 60 seconds → 1m
        assert_eq!(App::format_elapsed(60), "Updated 1m ago");
        // 3599 seconds → 59m
        assert_eq!(App::format_elapsed(3599), "Updated 59m ago");
        // 3600 seconds → 1h
        assert_eq!(App::format_elapsed(3600), "Updated 1h ago");
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

    // --- Font / encoding related tests ---

    #[test]
    fn test_font_definitions_default_has_font_data() {
        let fonts = egui::FontDefinitions::default();
        assert!(!fonts.font_data.is_empty(), "Default font definitions should contain font data");
        assert!(!fonts.families.is_empty(), "Default font definitions should have font families");
    }

    #[test]
    fn test_font_definitions_proportional_has_fallback() {
        let fonts = egui::FontDefinitions::default();
        let prop = fonts.families.get(&egui::FontFamily::Proportional);
        assert!(prop.is_some(), "Proportional font family should exist");
        let prop = prop.unwrap();
        assert!(!prop.is_empty(), "Proportional family should have at least one font");
    }

    #[test]
    fn test_font_definitions_monospace_family() {
        let fonts = egui::FontDefinitions::default();
        let mono = fonts.families.get(&egui::FontFamily::Monospace);
        assert!(mono.is_some(), "Monospace font family should exist");
        let mono = mono.unwrap();
        assert!(!mono.is_empty(), "Monospace family should have at least one font");
    }

    #[test]
    fn test_font_data_support_emoji_range() {
        let rocket_emoji = "🚀";
        assert_eq!(rocket_emoji.len(), 4, "Rocket emoji should be 4 bytes in UTF-8");
        assert!(rocket_emoji.chars().all(|c| c.is_ascii() || c as u32 > 127),
            "Emoji characters should be valid Unicode");
    }

    #[test]
    fn test_unicode_arrows_are_valid_utf8() {
        let up_arrow = '↑'; // U+2191
        let down_arrow = '↓'; // U+2193
        let play_icon = '▶'; // U+25B6
        assert_eq!(up_arrow as u32, 0x2191, "↑ should be U+2191");
        assert_eq!(down_arrow as u32, 0x2193, "↓ should be U+2193");
        assert_eq!(play_icon as u32, 0x25B6, "▶ should be U+25B6");
        let s = format!("{} {} {}", up_arrow, down_arrow, play_icon);
        assert_eq!(s.chars().count(), 5, "String should contain 5 chars (3 symbols + 2 spaces)");
    }

    #[test]
    fn test_emoji_chars_in_app_ui() {
        let emojis = ['📂', '🔀', '📋', '📦', '🌐', '▶', 'ⓘ', '🔄', '🗑', '⏳', '📊'];
        for (i, &emoji) in emojis.iter().enumerate() {
            assert!(emoji as u32 > 127, "Emoji {} (index {}) should be a Unicode character", emoji, i);
        }
    }

    #[test]
    fn test_log_tab_uses_clipboard_emoji_not_alarm_clock() {
        let tabs = [
            (Tab::Status, "📊 Status"),
            (Tab::Branches, "🔀 Branches"),
            (Tab::Worktrees, "📂 Worktrees"),
            (Tab::Log, "📋 Log"),
            (Tab::Stash, "📦 Stash"),
            (Tab::Remotes, "🌐 Remotes"),
        ];
        let log_label = tabs.iter().find(|(t, _)| *t == Tab::Log).map(|(_, l)| *l).unwrap();
        assert!(
            !log_label.contains('\u{23F0}'),
            "Log tab must NOT use ⏰ (alarm clock) which renders as a box. Found: {}",
            log_label
        );
        assert!(
            log_label.contains("📋"),
            "Log tab should use 📋 (clipboard) emoji. Found: {}",
            log_label
        );
    }

    #[test]
    fn test_about_button_does_not_use_circled_i() {
        let bad_char = '\u{24D8}';
        let about_labels = ["ℹ", "About"];
        for label in &about_labels {
            assert!(
                !label.contains(bad_char),
                "About button label '{}' must NOT use ⓘ which renders as a box",
                label
            );
        }
    }

    #[test]
    fn test_repo_name_extracts_last_path_component() {
        let cases = [
            ("C:\\Users\\me\\projects\\my-project", "my-project"),
            ("/home/user/projects/my-repo", "my-repo"),
            ("/a/b/c", "c"),
            ("just-a-name", "just-a-name"),
            ("", ""),
        ];
        for (path, expected) in &cases {
            let mut app = App::new();
            app.repo_path = path.to_string();
            assert_eq!(app.repo_name(), *expected, "repo_name() for path '{}'", path);
        }
    }

    #[test]
    fn test_repo_name_empty_when_no_repo_open() {
        let app = App::new();
        assert_eq!(app.repo_name(), "", "repo_name should be empty when no repo is open");
    }

    #[test]
    fn test_pending_op_progress_sharing() {
        use std::sync::{Arc, Mutex};
        let progress = Arc::new(Mutex::new(String::new()));

        {
            let p = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(10));
                *p.lock().unwrap() = "Receiving objects: 45%".to_string();
            });
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
        let current = progress.lock().unwrap().clone();
        assert_eq!(current, "Receiving objects: 45%");
    }

    #[test]
    fn test_pending_op_progress_empty_initially() {
        use std::sync::{Arc, Mutex};
        let progress = Arc::new(Mutex::new(String::new()));
        assert!(progress.lock().unwrap().is_empty());
    }

    #[test]
    fn test_current_operation_empty() {
        let app = App::new();
        assert_eq!(app.current_operation(), "");
    }

    #[test]
    fn test_is_busy_initially_false() {
        let app = App::new();
        assert!(!app.is_busy());
    }

    #[test]
    fn test_pending_op_contains_progress() {
        use std::sync::{Arc, Mutex};
        let op = PendingOp {
            description: "Fetch from origin".to_string(),
            receiver: mpsc::channel::<OpResult>().1,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new("initial progress".to_string())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
        };
        assert_eq!(*op.progress.lock().unwrap(), "initial progress");
    }

    #[test]
    fn test_format_elapsed_no_panic_on_large_values() {
        let result = App::format_elapsed(u64::MAX);
        assert!(!result.is_empty());
        assert!(result.contains("h ago"));
    }

    // --- Update dialog dismiss flag tests ---

    #[test]
    fn test_update_dialog_dismissed_initially_false() {
        let app = App::new();
        assert!(!app.update_dialog_dismissed, "Dismiss flag should start as false");
    }

    #[test]
    fn test_trigger_update_check_resets_dismiss_flag() {
        let mut app = App::new();
        app.update_dialog_dismissed = true;
        app.trigger_update_check();
        assert!(!app.update_dialog_dismissed, "Triggering a new check should reset dismiss flag");
    }

    #[test]
    fn test_dismiss_flag_prevents_dialog_reopen() {
        let mut app = App::new();
        app.update_dialog_dismissed = true;
        app.show_update_dialog = false;

        if !app.update_dialog_dismissed {
            if !app.show_update_dialog {
                app.show_update_dialog = true;
            }
        }

        assert!(!app.show_update_dialog, "Dialog should not reopen when dismissed");
    }

    #[test]
    fn test_dialog_opens_when_not_dismissed() {
        let mut app = App::new();
        app.update_dialog_dismissed = false;
        app.show_update_dialog = false;

        if !app.update_dialog_dismissed {
            if !app.show_update_dialog {
                app.show_update_dialog = true;
            }
        }

        assert!(app.show_update_dialog, "Dialog should open when not dismissed");
    }

    #[test]
    fn test_dismiss_flag_after_remind_later() {
        let mut app = App::new();
        app.show_update_dialog = false;
        app.update_dialog_dismissed = true;

        assert!(app.update_dialog_dismissed, "Remind Later should set dismiss flag");
        assert!(!app.show_update_dialog, "Remind Later should close dialog");
    }

    // --- Download tracking tests ---

    #[test]
    fn test_download_progress_field_defaults() {
        let app = App::new();
        assert_eq!(app.download_progress, 0.0, "Download progress should start at 0");
    }

    // --- Status bar truncation & overwrite tests ---

    #[test]
    fn test_show_message_popup_initially_false() {
        let app = App::new();
        assert!(!app.show_message_popup);
    }

    #[test]
    fn test_flush_messages_overwrites_error() {
        let mut app = App::new();
        app.pending_errors.push("first error".into());
        app.flush_messages();
        assert_eq!(app.error_message, "first error");
        assert!(app.pending_errors.is_empty());

        app.pending_errors.push("second error".into());
        app.flush_messages();
        assert_eq!(
            app.error_message, "second error",
            "flush_messages should overwrite, not append, old error_message"
        );
    }

    #[test]
    fn test_flush_messages_overwrites_multiple_errors_with_single_message() {
        let mut app = App::new();
        app.pending_errors.push("error one".into());
        app.pending_errors.push("error two".into());
        app.flush_messages();
        assert_eq!(app.error_message, "error one | error two");

        app.pending_errors.push("fresh error".into());
        app.flush_messages();
        assert_eq!(
            app.error_message, "fresh error",
            "After overwrite, should only contain the new error, not accumulated"
        );
    }

    #[test]
    fn test_flush_messages_success_works_independently() {
        let mut app = App::new();
        app.pending_successes.push("success!".into());
        app.flush_messages();
        assert_eq!(app.success_message, "success!");
        assert!(app.error_message.is_empty());
    }

    #[test]
    fn test_flush_messages_does_not_clear_error_when_no_pending_errors() {
        let mut app = App::new();
        app.error_message = "existing error".into();
        app.flush_messages();
        assert_eq!(app.error_message, "existing error");
    }

    #[test]
    fn test_flush_messages_success_multiple_joined() {
        let mut app = App::new();
        app.pending_successes.push("done".into());
        app.pending_successes.push("pushed".into());
        app.flush_messages();
        assert_eq!(app.success_message, "done | pushed");
    }

    #[test]
    fn test_show_error_overwrites() {
        let mut app = App::new();
        app.error_message = "old error".into();
        app.show_error("new error".into());
        assert_eq!(app.error_message, "new error");
    }

    #[test]
    fn test_message_popup_store_full_text() {
        let mut app = App::new();
        app.show_error("detailed error message".into());
        app.show_success("detailed success message".into());
        assert!(!app.show_message_popup);
        assert_eq!(app.error_message, "detailed error message");
        assert_eq!(app.success_message, "detailed success message");
    }
}
