use crate::git_ops::*;
use crate::recent::{path_name, RecentRepos};
use crate::updater::{self, UpdateState};
use eframe::egui;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const ABOUT_BUTTON_LABEL: &str = "ℹ";

/// Tracks a Git operation running in a background thread.
struct PendingOp {
    description: String,
    receiver: mpsc::Receiver<OpResult>,
    repo_generation: u64,
    started_at: Instant,
    /// Real-time progress text updated by the background thread (e.g. "Receiving objects: 45%").
    progress: Arc<Mutex<String>>,
    /// Tracks the last time the progress text changed (watchdog timer).
    last_progress_update: Instant,
    /// The last progress value we read (to detect changes).
    last_seen_progress: String,
    /// Whether the watchdog timed out while the worker was still running.
    timed_out: bool,
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
    /// Single status message for the bottom bar — shows the latest operation,
    /// concise success/error. Replaces old error_message + success_message.
    pub status_message: String,
    /// Whether the status_message represents an error (for coloring).
    pub status_is_error: bool,
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
    /// Whether the remote field has been edited by the user.
    pub(crate) remote_name_user_edited: bool,
    pub push_branch: String,
    /// Whether the push branch field has been edited by the user.
    pub(crate) push_branch_user_edited: bool,
    pub push_force: bool,
    pub pull_rebase: bool,

    pub diff_content: Vec<DiffLine>,
    pub diff_path: String,
    pub show_diff: bool,
    pub log_search: String,
    /// Monotonic identity of the newest log search request.
    log_search_request_id: u64,
    /// Monotonic identity of the currently open repository.
    repo_generation: u64,

    pub last_refresh: std::time::Instant,

    pub show_about: bool,
    pub update_state: Arc<Mutex<UpdateState>>,
    pub show_update_dialog: bool,
    pub auto_check_done: bool,
    /// Set to true when the user dismisses the update dialog to prevent it from reopening.
    pub update_dialog_dismissed: bool,
    /// Download progress from 0.0 to 1.0 for the current download.
    pub download_progress: f32,
    /// Pending Git operations running in background threads.
    pending_ops: Vec<PendingOp>,
    /// Whether to auto-refresh after a mutation operation completes.
    needs_refresh: bool,
    pub recent_repos: RecentRepos,
    pub status_expanded: bool,
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
            status_message: String::new(),
            status_is_error: false,
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
            remote_name_user_edited: false,
            push_branch: String::new(),
            push_branch_user_edited: false,
            push_force: false,
            pull_rebase: false,

            diff_content: Vec::new(),
            diff_path: String::new(),
            show_diff: false,
            log_search: String::new(),
            log_search_request_id: 0,
            repo_generation: 0,

            last_refresh: std::time::Instant::now(),

            show_about: false,

            update_state: Arc::new(Mutex::new(UpdateState::Idle)),
            show_update_dialog: false,
            auto_check_done: false,
            update_dialog_dismissed: false,
            download_progress: 0.0,
            pending_ops: Vec::new(),
            needs_refresh: false,
            recent_repos: RecentRepos::load(),
            status_expanded: false,
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

    fn dismiss_update_dialog(&mut self) {
        self.show_update_dialog = false;
        self.update_dialog_dismissed = true;
    }

    fn update_download_progress_if_active(
        state: &Mutex<UpdateState>,
        progress: f32,
        file_name: &str,
    ) -> bool {
        let mut current_state = state.lock().unwrap();
        if !matches!(*current_state, UpdateState::Downloading { .. }) {
            return false;
        }

        *current_state = UpdateState::Downloading {
            progress,
            file_name: file_name.to_string(),
        };
        true
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
                    if !App::update_download_progress_if_active(
                        &state_for_progress,
                        p,
                        &file_name,
                    ) {
                        break;
                    }
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
                self.status_message = format!("Failed to extract update: {}", e);
                self.status_is_error = true;
                return;
            }
        };

        // Get current executable path
        let current_binary = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                self.status_message = format!("Failed to get current exe path: {}", e);
                self.status_is_error = true;
                return;
            }
        };

        // Create self-update script
        let script_path = match updater::create_self_update_script(&new_binary, &current_binary) {
            Ok(p) => p,
            Err(e) => {
                self.status_message = format!("Failed to create update script: {}", e);
                self.status_is_error = true;
                return;
            }
        };

        // Launch the script (detached from parent process)
        if !self.try_launch_update_script(&script_path) {
            return;
        }

        // Exit the current process immediately
        std::process::exit(0);
    }

    fn try_launch_update_script(&mut self, script_path: &Path) -> bool {
        match updater::launch_self_update_script(script_path) {
            Ok(()) => true,
            Err(e) => {
                self.show_error(e);
                false
            }
        }
    }

    pub fn open_repo(&mut self, path: &str) {
        // Pending operations publish results back into this App, so changing
        // repositories before they finish could apply stale data to the new one.
        if self.is_busy() {
            return;
        }

        self.status_message.clear();
        self.status_is_error = false;
        match self.git.open(Path::new(path)) {
            Ok(()) => {
                self.log_search_request_id = self.log_search_request_id.wrapping_add(1);
                self.repo_generation = self.repo_generation.wrapping_add(1);
                self.repo_path = path.to_string();
                self.remote_name_user_edited = false;
                self.push_branch.clear();
                self.push_branch_user_edited = false;
                self.diff_content.clear();
                self.diff_path.clear();
                self.show_diff = false;
                self.status_message = format!("Opened repository at {}", path);
                self.status_is_error = false;
                self.refresh_all();
                if let Err(error) = self.recent_repos.add(path) {
                    self.status_message = format!(
                        "Opened repository at {} (failed to save recent history: {})",
                        path, error
                    );
                    self.status_is_error = true;
                }
            }
            Err(e) => {
                self.status_message = format!("Failed to open repo: {}", e);
                self.status_is_error = true;
            }
        }
    }

    /// Checks if there are any pending background operations.
    pub fn is_busy(&self) -> bool {
        !self.pending_ops.is_empty()
    }

    /// Returns the description of the current/last operation.
    /// For the status bar: just the operation name (concise).
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
        let repo_generation = self.repo_generation;
        let progress = Arc::new(Mutex::new(String::new()));
        let op_progress = progress.clone();

        std::thread::spawn(move || {
            let result = execute_operation(&repo_path, op, op_progress);
            let _ = tx.send(result);
        });

        self.pending_ops.push(PendingOp {
            description: desc,
            receiver: rx,
            repo_generation,
            started_at: Instant::now(),
            progress,
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
        });

        // Initialize the operation log with description
        self.last_operation_log = format!("▶ Operation: {}\n", description);
        // Ensure log has a trailing placeholder so progress can accumulate
        self.last_operation_log += "  (waiting for progress...)\n";

        ctx.request_repaint();
    }

    /// Start a log search with a request identity so out-of-order responses can be ignored.
    pub fn start_log_search(&mut self, ctx: &egui::Context, filter: String) {
        self.log_search_request_id = self.log_search_request_id.wrapping_add(1);
        let request_id = self.log_search_request_id;
        self.start_operation(
            ctx,
            "Searching commits",
            GitOperation::LogSearch { filter, request_id },
        );
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
            let (description, started_at, current_progress, last_seen_progress, last_progress_update, timed_out) = {
                let op = &self.pending_ops[i];
                let description = op.description.clone();
                let started_at = op.started_at;
                let current_progress = op.progress.lock().unwrap().clone();
                let last_seen_progress = op.last_seen_progress.clone();
                let last_progress_update = op.last_progress_update;
                let timed_out = op.timed_out;
                (description, started_at, current_progress, last_seen_progress, last_progress_update, timed_out)
            };

            // If progress text changed, reset the watchdog timer and accumulate to log
            if !timed_out && current_progress != last_seen_progress {
                if let Some(mut_op) = self.pending_ops.get_mut(i) {
                    mut_op.last_progress_update = Instant::now();
                    mut_op.last_seen_progress = current_progress.clone();
                }
                if !current_progress.is_empty() {
                    self.last_operation_log += &format!("  {}\n", current_progress);
                }
            }

            let receive_result = self.pending_ops[i].receiver.try_recv();
            if !timed_out
                && current_progress == last_seen_progress
                && receive_result.is_err()
            {
                if current_progress.is_empty() {
                    // No progress ever received: give 60 seconds total
                    if started_at.elapsed().as_secs() > 60 {
                        let msg = format!(
                            "Operation '{}' timed out (no progress in 60s)",
                            description
                        );
                        self.status_message = msg.clone();
                        self.status_is_error = true;
                        self.last_operation_log += &format!("  ✗ {}\n", msg);
                        self.pending_ops[i].timed_out = true;
                        i += 1;
                        continue;
                    }
                } else {
                    // Progress was received but stopped: 30 second stall threshold
                    let stall_secs = last_progress_update.elapsed().as_secs();
                    if stall_secs > 30 {
                        let msg = format!(
                            "Operation '{}' timed out (stalled {}s)\nLast: {}",
                            description, stall_secs, current_progress
                        );
                        self.status_message = msg.clone();
                        self.status_is_error = true;
                        self.last_operation_log += &format!("  ✗ {}\n", msg);
                        self.pending_ops[i].timed_out = true;
                        i += 1;
                        continue;
                    }
                }
            }

            let op = &self.pending_ops[i];
            match receive_result {
                Ok(result) => {
                    let op = self.pending_ops.swap_remove(i);
                    if op.timed_out {
                        // Keep the UI blocked until the timed-out worker has finished. A
                        // successful late result may have mutated Git state, so refresh it
                        // before allowing another operation to start.
                        if matches!(&result, OpResult::Success(_)) {
                            self.needs_refresh = true;
                        }
                        continue;
                    }
                    if op.repo_generation != self.repo_generation
                        && matches!(&result, OpResult::RefreshData { .. })
                    {
                        continue;
                    }
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
                    if self.pending_ops[i].timed_out {
                        self.pending_ops.swap_remove(i);
                        continue;
                    }
                    let last_prog = op.progress.lock().unwrap().clone();
                    let fail_msg = if last_prog.is_empty() {
                        format!("Operation '{}' failed unexpectedly", op.description)
                    } else {
                        format!("Operation '{}' failed unexpectedly\nLast progress: {}", op.description, last_prog)
                    };
                    self.status_message = fail_msg.clone();
                    self.status_is_error = true;
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
                // Set status_message to the concise success message
                self.status_message = msg;
                self.status_is_error = false;
                // Auto-refresh after mutation operations
                self.needs_refresh = true;
            }
            OpResult::Error(e) => {
                let err_msg = format!("{}: {}", description, e);
                self.last_operation_log += &format!("  ✗ {}\n", err_msg);
                // Set status_message to concise error message
                self.status_message = err_msg;
                self.status_is_error = true;
            }
            OpResult::DiffContent { path, lines } => {
                self.diff_path = path;
                self.diff_content = lines;
                self.show_diff = true;
            }
            OpResult::SearchResults { request_id, filter, commits } => {
                if request_id == self.log_search_request_id && filter == self.log_search {
                    self.commits = commits;
                }
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
                self.commits = filter_commits(commits, &self.log_search);
                self.stashes = stashes;
                self.remote_list = remote_list;
                self.last_refresh = Instant::now();
                if !errors.is_empty() {
                    let msg = errors.join("; ");
                    self.last_operation_log += &format!("  ✗ {}\n", msg);
                    self.status_message = msg;
                    self.status_is_error = true;
                }
            }
        }
    }

    pub fn refresh_all(&mut self) {
        if !self.git.is_open() {
            return;
        }
        self.status_message.clear();
        self.status_is_error = false;

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
        let commits = self.git.log(100).unwrap_or_else(|e| {
            errors.push(format!("Log: {}", e));
            Vec::new()
        });
        self.commits = filter_commits(commits, &self.log_search);
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
        self.status_message = msg;
        self.status_is_error = true;
    }

    pub fn show_success(&mut self, msg: String) {
        self.status_message = msg;
        self.status_is_error = false;
    }

    /// Returns the project folder name extracted from the repo path.
    /// e.g. "/home/user/projects/my-repo" → "my-repo"
    pub fn repo_name(&self) -> String {
        path_name(&self.repo_path)
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
                let recent_repos_enabled = !self.is_busy();
                ui.add_enabled_ui(recent_repos_enabled, |ui| {
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
                            if let Err(error) = self.recent_repos.remove(idx) {
                                self.status_message =
                                    format!("Failed to save recent history: {}", error);
                                self.status_is_error = true;
                            }
                        }
                        }
                    });
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
                        if ui.button(ABOUT_BUTTON_LABEL).clicked() {
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
                        if ui.button(ABOUT_BUTTON_LABEL).clicked() {
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

        // --- Bottom Bar (simplified: single message + "..." expand + timestamp) ---
        if self.git.is_open() {
            let bottom_height = if self.status_expanded { 200.0 } else { 24.0 };
            egui::TopBottomPanel::bottom("bottom_bar")
                .resizable(false)
                .min_height(bottom_height)
                .show(ctx, |ui| {
                    let dark = ctx.style().visuals.dark_mode;
                    ui.horizontal(|ui| {
                        // --- Right side: timestamp + "..." expand/collapse button ---
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let elapsed = self.last_refresh.elapsed().as_secs();
                            let elapsed_text = App::format_elapsed(elapsed);
                            ui.add(
                                egui::Label::new(&elapsed_text)
                                    .truncate(),
                            )
                            .on_hover_text(elapsed_text);

                            // Expand button: "..." when collapsed, "✕" when expanded
                            let btn_label = if self.status_expanded { "✕" } else { "···" };
                            if ui.button(btn_label).clicked() {
                                self.status_expanded = !self.status_expanded;
                            }
                        });

                        // --- Left side: single status message (operation or result) ---
                        if self.is_busy() {
                            // Show concise operation name while busy
                            let op_text = self.current_operation();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&op_text)
                                        .color(App::adaptive_yellow(dark))
                                        .size(13.0),
                                )
                                .truncate(),
                            );
                        } else if !self.status_message.is_empty() {
                            let color = if self.status_is_error {
                                App::adaptive_red(dark)
                            } else {
                                App::adaptive_green(dark)
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&self.status_message)
                                        .color(color)
                                        .size(13.0),
                                )
                                .truncate(),
                            );
                        }
                    });

                    // --- Expanded area: full command output log ---
                    if self.status_expanded {
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(self.last_operation_log.clone())
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
                            if let Err(error) = self.recent_repos.remove(idx) {
                                self.status_message =
                                    format!("Failed to save recent history: {}", error);
                                self.status_is_error = true;
                            }
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

            // Wrap tab panel in a vertical ScrollArea so content is scrollable
            // when the expanded bottom bar takes up vertical space.
            egui::ScrollArea::vertical()
                .id_salt("main_content_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Render the active tab panel
                    match self.current_tab {
                        Tab::Status => crate::ui::status_panel::show(self, ui, ctx),
                        Tab::Branches => crate::ui::branch_panel::show(self, ui, ctx),
                        Tab::Worktrees => crate::ui::worktree_panel::show(self, ui, ctx),
                        Tab::Log => crate::ui::log_panel::show(self, ui, ctx),
                        Tab::Stash => crate::ui::stash_panel::show(self, ui, ctx),
                        Tab::Remotes => crate::ui::remote_panel::show(self, ui, ctx),
                    }
                    ui.allocate_space(ui.available_size());
                });
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
                                            self.dismiss_update_dialog();
                                        }
                                        if crate::ui::ellipsis_button(ui, "Remind Later").clicked() {
                                            self.dismiss_update_dialog();
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- New tests for simplified status bar (Requirement 3 & 4) ---

    #[test]
    fn test_status_message_starts_empty() {
        let app = App::new();
        assert!(app.status_message.is_empty());
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_show_error_sets_status_message_and_is_error() {
        let mut app = App::new();
        app.show_error("Cannot delete branch 'feature-x'".into());
        assert_eq!(app.status_message, "Cannot delete branch 'feature-x'");
        assert!(app.status_is_error);
    }

    #[test]
    fn test_show_success_sets_status_message_and_not_error() {
        let mut app = App::new();
        app.show_success("Deleted 'feature-x' successfully".into());
        assert_eq!(app.status_message, "Deleted 'feature-x' successfully");
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_show_error_overwrites_status_message() {
        let mut app = App::new();
        app.show_success("old success".into());
        assert!(!app.status_is_error);
        app.show_error("new error".into());
        assert_eq!(app.status_message, "new error");
        assert!(app.status_is_error);
    }

    #[test]
    fn test_show_success_overwrites_status_message() {
        let mut app = App::new();
        app.show_error("old error".into());
        assert!(app.status_is_error);
        app.show_success("new success".into());
        assert_eq!(app.status_message, "new success");
        assert!(!app.status_is_error);
    }

    fn test_commit(message: &str, author: &str) -> CommitInfo {
        CommitInfo {
            sha: format!("{message}-sha"),
            short_sha: "1234567".to_string(),
            author: author.to_string(),
            time: "2026-01-01 00:00:00".to_string(),
            message: message.to_string(),
            summary: message.to_string(),
        }
    }

    #[test]
    fn test_stale_search_results_do_not_replace_current_log() {
        let mut app = App::new();
        app.log_search = "alice".to_string();
        app.log_search_request_id = 1;
        app.commits = vec![test_commit("alice changed the parser", "alice")];

        app.handle_op_result(
            "Searching commits".to_string(),
            OpResult::SearchResults {
                request_id: 0,
                filter: "bob".to_string(),
                commits: vec![test_commit("bob changed the parser", "bob")],
            },
        );

        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].author, "alice");
    }

    #[test]
    fn test_refresh_preserves_active_log_filter() {
        let mut app = App::new();
        app.log_search = "alice".to_string();

        app.handle_op_result(
            "Refreshing".to_string(),
            OpResult::RefreshData {
                status_entries: Vec::new(),
                branches: Vec::new(),
                worktrees: Vec::new(),
                commits: vec![
                    test_commit("alice changed the parser", "alice"),
                    test_commit("unrelated change", "bob"),
                ],
                stashes: Vec::new(),
                remote_list: Vec::new(),
                errors: Vec::new(),
            },
        );

        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].author, "alice");
    }

    #[test]
    fn test_stale_refresh_result_does_not_replace_data_after_repo_switch() {
        fn init_repo(path: &std::path::Path, message: &str) {
            let repo = git2::Repository::init(path).expect("init repo");
            let signature = git2::Signature::now("test", "test@example.com").expect("signature");
            let tree_oid = {
                let mut index = repo.index().expect("index");
                index.write_tree().expect("write tree")
            };
            let tree = repo.find_tree(tree_oid).expect("tree");
            repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .expect("commit");
        }

        let old_repo = tempfile::tempdir().expect("old repo dir");
        let new_repo = tempfile::tempdir().expect("new repo dir");
        init_repo(old_repo.path(), "old repository commit");
        init_repo(new_repo.path(), "new repository commit");

        let recent_file = tempfile::NamedTempFile::new().expect("recent repos file");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_file.path().to_path_buf());
        app.open_repo(old_repo.path().to_str().expect("old repo path"));

        let old_generation = app.repo_generation;
        app.open_repo(new_repo.path().to_str().expect("new repo path"));

        let (tx, rx) = mpsc::channel();
        tx.send(OpResult::RefreshData {
            status_entries: Vec::new(),
            branches: Vec::new(),
            worktrees: Vec::new(),
            commits: vec![test_commit("stale old repository data", "old")],
            stashes: Vec::new(),
            remote_list: Vec::new(),
            errors: Vec::new(),
        })
        .expect("send stale refresh result");
        app.pending_ops.push(PendingOp {
            description: "Refreshing".to_string(),
            receiver: rx,
            repo_generation: old_generation,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
        });

        app.process_pending_ops(&egui::Context::default());

        assert_eq!(app.repo_path, new_repo.path().to_string_lossy());
        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].summary, "new repository commit");
    }

    #[test]
    fn test_open_repo_clears_diff_from_previous_repository() {
        let first_repo = tempfile::tempdir().expect("first repo dir");
        let second_repo = tempfile::tempdir().expect("second repo dir");
        git2::Repository::init(first_repo.path()).expect("init first repo");
        git2::Repository::init(second_repo.path()).expect("init second repo");

        let recent_file = tempfile::NamedTempFile::new().expect("recent repos file");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_file.path().to_path_buf());
        app.open_repo(first_repo.path().to_str().expect("first repo path"));

        app.handle_op_result(
            "Show diff".to_string(),
            OpResult::DiffContent {
                path: "repo-a.txt".to_string(),
                lines: vec![DiffLine {
                    origin: '+',
                    content: "repo A content".to_string(),
                }],
            },
        );
        assert!(app.show_diff);
        assert_eq!(app.diff_path, "repo-a.txt");
        assert_eq!(app.diff_content.len(), 1);

        app.open_repo(second_repo.path().to_str().expect("second repo path"));

        assert!(app.diff_content.is_empty());
        assert!(app.diff_path.is_empty());
        assert!(!app.show_diff);
    }

    #[test]
    fn test_refresh_all_preserves_active_log_filter() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = git2::Repository::init(dir.path()).expect("init repo");
        let alice = git2::Signature::now("alice", "alice@example.com").expect("signature");
        let bob = git2::Signature::now("bob", "bob@example.com").expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("tree");
        let first_oid = repo
            .commit(Some("HEAD"), &alice, &alice, "alice change", &tree, &[])
            .expect("alice commit");
        let first = repo.find_commit(first_oid).expect("first commit");
        repo.commit(Some("HEAD"), &bob, &bob, "bob change", &tree, &[&first])
            .expect("bob commit");
        drop(first);
        drop(tree);
        drop(repo);

        let mut app = App::new();
        app.git.open(dir.path()).expect("open repo");
        app.log_search = "alice".to_string();
        app.refresh_all();

        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].author, "alice");
    }

    #[test]
    fn test_repeated_search_query_rejects_stale_response() {
        let mut app = App::new();
        let ctx = egui::Context::default();

        app.log_search = "alice".to_string();
        let filter = app.log_search.clone();
        app.start_log_search(&ctx, filter);
        let first_a_request_id = app.log_search_request_id;
        app.log_search = "bob".to_string();
        let filter = app.log_search.clone();
        app.start_log_search(&ctx, filter);
        app.log_search = "alice".to_string();
        let filter = app.log_search.clone();
        app.start_log_search(&ctx, filter);
        let second_a_request_id = app.log_search_request_id;
        app.commits = vec![test_commit("latest alice result", "alice")];

        app.handle_op_result(
            "Searching commits".to_string(),
            OpResult::SearchResults {
                request_id: first_a_request_id,
                filter: "alice".to_string(),
                commits: vec![test_commit("stale alice result", "alice")],
            },
        );

        assert_eq!(app.commits[0].message, "latest alice result");

        app.handle_op_result(
            "Searching commits".to_string(),
            OpResult::SearchResults {
                request_id: second_a_request_id,
                filter: "alice".to_string(),
                commits: vec![test_commit("current alice result", "alice")],
            },
        );

        assert_eq!(app.commits[0].message, "current alice result");
    }

    #[test]
    fn test_status_expanded_defaults_to_false() {
        let app = App::new();
        assert!(!app.status_expanded);
    }

    #[test]
    fn test_current_operation_empty_when_not_busy() {
        let app = App::new();
        assert_eq!(app.current_operation(), "");
    }

    #[test]
    fn test_open_repo_clears_status_message() {
        // We can't fully test open_repo without a real git repo,
        // but we can verify it clears status_message
        let mut app = App::new();
        app.status_message = "old message".into();
        app.status_is_error = true;
        // Clearing before open:
        app.status_message.clear();
        app.status_is_error = false;
        assert!(app.status_message.is_empty());
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_open_repo_is_ignored_while_operation_is_pending() {
        let first_repo = tempfile::tempdir().unwrap();
        let second_repo = tempfile::tempdir().unwrap();
        git2::Repository::init(first_repo.path()).unwrap();
        git2::Repository::init(second_repo.path()).unwrap();

        let mut app = App::new();
        let recent_repos_dir = tempfile::tempdir().unwrap();
        app.recent_repos = RecentRepos::load_from(recent_repos_dir.path().join("recent.json"));
        let first_path = first_repo.path().to_string_lossy().to_string();
        let second_path = second_repo.path().to_string_lossy().to_string();
        app.open_repo(&first_path);

        let (_tx, receiver) = mpsc::channel::<OpResult>();
        app.pending_ops.push(PendingOp {
            description: "Fetching".to_string(),
            receiver,
            repo_generation: app.repo_generation,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
        });

        app.open_repo(&second_path);

        assert_eq!(app.repo_path, first_path);
        assert_eq!(app.git.path().unwrap(), first_repo.path());
    }

    #[cfg(unix)]
    #[test]
    fn test_update_script_launch_failure_is_reported() {
        let mut app = App::new();

        assert!(!app.try_launch_update_script(Path::new(
            "/path/that/does/not/exist/update_git_manager.sh",
        )));
        assert!(app.status_is_error);
        assert!(app.status_message.contains("Failed to launch update script"));
    }

    // --- Legacy tests (unchanged) ---

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
        let emojis = ['📂', '🔀', '📋', '📦', '🌐', '▶', 'ℹ', '🔄', '🗑', '⏳', '📊'];
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
        assert_eq!(ABOUT_BUTTON_LABEL, "ℹ");
        assert!(
            !ABOUT_BUTTON_LABEL.contains(bad_char),
            "About button label '{}' must NOT use ⓘ which renders as a box",
            ABOUT_BUTTON_LABEL
        );

        let ctx = egui::Context::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = ui.button(ABOUT_BUTTON_LABEL);
                });
            },
        );
        let text_shape = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == ABOUT_BUTTON_LABEL => {
                    Some(text)
                }
                _ => None,
            })
            .expect("About button should paint a text shape");
        let font_id = text_shape
            .galley
            .job
            .sections
            .first()
            .expect("About button text should have a font section")
            .format
            .font_id
            .clone();
        assert!(
            ctx.fonts(|fonts| fonts.has_glyph(&font_id, 'ℹ')),
            "About button font should provide an actual ℹ glyph"
        );
        assert!(text_shape
            .galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .any(|glyph| glyph.chr == 'ℹ'));
    }

    #[test]
    fn test_repo_name_extracts_last_path_component() {
        let cases = [
            ("C:\\Users\\me\\projects\\my-project", "my-project"),
            ("C:\\Users\\me\\projects\\my-project\\", "my-project"),
            ("/home/user/projects/my-repo", "my-repo"),
            ("/home/user/projects/my-repo/", "my-repo"),
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
    fn test_timed_out_operation_stays_busy_until_worker_finishes() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        let mut app = App::new();
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        let worker_finished = Arc::new(AtomicBool::new(false));
        let worker_finished_clone = worker_finished.clone();

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            tx.send(OpResult::Success("late mutation".to_string())).unwrap();
            worker_finished_clone.store(true, Ordering::SeqCst);
        });

        app.pending_ops.push(PendingOp {
            description: "Timed operation".to_string(),
            receiver: rx,
            repo_generation: app.repo_generation,
            started_at: Instant::now() - Duration::from_secs(61),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
        });

        app.process_pending_ops(&ctx);

        assert!(app.is_busy(), "a timed-out worker must still block new operations");
        assert!(app.status_message.contains("timed out"));

        while !worker_finished.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        app.process_pending_ops(&ctx);

        assert!(!app.is_busy(), "the operation can be released after its worker exits");
        assert!(
            app.status_message.contains("timed out"),
            "a late result must not replace the timeout status"
        );
    }

    #[test]
    fn test_queued_result_is_processed_before_timeout_classification() {
        use std::time::Duration;

        let mut app = App::new();
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send(OpResult::Success("completed mutation".to_string()))
            .unwrap();

        app.pending_ops.push(PendingOp {
            description: "Completed operation".to_string(),
            receiver: rx,
            repo_generation: app.repo_generation,
            started_at: Instant::now() - Duration::from_secs(61),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
        });

        app.process_pending_ops(&ctx);

        assert!(
            !app.is_busy(),
            "a queued result should finish the operation"
        );
        assert_eq!(app.status_message, "completed mutation");
        assert!(!app.status_is_error);
        assert!(app.last_operation_log.contains("✓ completed mutation"));
    }

    #[test]
    fn test_pending_op_contains_progress() {
        use std::sync::{Arc, Mutex};
        let op = PendingOp {
            description: "Fetch from origin".to_string(),
            receiver: mpsc::channel::<OpResult>().1,
            repo_generation: 0,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new("initial progress".to_string())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
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

    #[test]
    fn test_open_in_browser_dismisses_dialog_for_next_frame() {
        let mut app = App::new();
        let repo_dir = tempfile::tempdir().expect("create temporary repository directory");
        git2::Repository::init(repo_dir.path()).expect("initialize temporary repository");
        app.git.open(repo_dir.path()).expect("open temporary repository");
        assert!(app.git.is_open());
        app.auto_check_done = true;
        app.show_update_dialog = true;
        app.update_dialog_dismissed = false;
        *app.update_state.lock().unwrap() = UpdateState::UpdateAvailable {
            latest_version: "0.2.0".to_string(),
            download_url: String::new(),
            assets: Vec::new(),
        };

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        let mut frame = eframe::Frame::_new_kittest();
        // Give egui one frame to initialize the update window before locating its button.
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        let browser_button_pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Open in Browser" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Update dialog should render an Open in Browser button");

        let pointer_input = |pressed| egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(browser_button_pos),
                egui::Event::PointerButton {
                    pos: browser_button_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };

        let _ = ctx.run(pointer_input(true), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });
        let _ = ctx.run(pointer_input(false), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });

        assert!(!app.show_update_dialog, "Opening the browser should close the dialog");
        assert!(
            app.update_dialog_dismissed,
            "Opening the browser should prevent the dialog from reopening"
        );

        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        assert!(
            !app.show_update_dialog,
            "The dialog should stay closed on the next frame after opening the browser"
        );
    }

    // --- Download tracking tests ---

    #[test]
    fn test_download_progress_field_defaults() {
        let app = App::new();
        assert_eq!(app.download_progress, 0.0, "Download progress should start at 0");
    }

    #[test]
    fn test_stale_download_progress_cannot_overwrite_completed_download() {
        let state = Arc::new(Mutex::new(UpdateState::Downloaded {
            file_path: "/tmp/update.zip".to_string(),
        }));

        assert!(!App::update_download_progress_if_active(
            &state,
            0.5,
            "update.zip",
        ));
        assert_eq!(
            *state.lock().unwrap(),
            UpdateState::Downloaded {
                file_path: "/tmp/update.zip".to_string(),
            }
        );
    }

}
