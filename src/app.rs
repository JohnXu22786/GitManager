use crate::git_ops::*;
use eframe::egui;
use std::path::Path;
use std::sync::mpsc;
use std::time::Instant;

/// Tracks a Git operation running in a background thread.
struct PendingOp {
    description: String,
    receiver: mpsc::Receiver<OpResult>,
    started_at: Instant,
}

#[derive(PartialEq, Clone)]
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

    /// Pending Git operations running in background threads.
    pending_ops: Vec<PendingOp>,
    /// Accumulated error messages from background operations.
    pending_errors: Vec<String>,
    /// Accumulated success messages from background operations.
    pending_successes: Vec<String>,
    /// Whether to auto-refresh after a mutation operation completes.
    needs_refresh: bool,
}

impl App {
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

            pending_ops: Vec::new(),
            pending_errors: Vec::new(),
            pending_successes: Vec::new(),
            needs_refresh: false,
        }
    }

    pub fn open_repo(&mut self, path: &str) {
        self.error_message.clear();
        self.success_message.clear();
        match self.git.open(Path::new(path)) {
            Ok(()) => {
                self.repo_path = path.to_string();
                self.success_message = format!("Opened repository at {}", path);
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

    fn status_label(origin: char) -> &'static str {
        match origin {
            ' ' | '+' => "",
            '-' => "del",
            _ => "mod",
        }
    }

    fn status_color(origin: char) -> egui::Color32 {
        match origin {
            '+' => egui::Color32::GREEN,
            '-' => egui::Color32::RED,
            _ => egui::Color32::GRAY,
        }
    }

    pub fn status_icon(s: char) -> &'static str {
        match s {
            'M' => "M",
            'A' => "A",
            'D' => "D",
            'R' => "R",
            'C' => "C",
            '?' => "?",
            '!' => "!",
            'U' => "U",
            _ => " ",
        }
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
        // --- Phase 1: Process any completed background operations ---
        self.process_pending_ops(ctx);
        self.flush_messages();

        let dark = ctx.style().visuals.dark_mode;
        if dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // --- Top Bar ---
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("📂");
                let open_btn = egui::Button::new("Open Repo...");
                if ui.add_enabled(!self.is_busy(), open_btn).clicked() {
                    let path = native_dialog_path();
                    if let Some(p) = path {
                        self.open_repo(&p);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("ℹ️").clicked() {
                        self.show_about = !self.show_about;
                    }
                });

                if self.git.is_open() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(&self.repo_path)
                            .color(egui::Color32::from_rgb(100, 150, 255)),
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
                        // Disable refresh button while busy
                        if ui.add_enabled(!self.is_busy(), egui::Button::new("🔄")).clicked() {
                            self.refresh_all();
                        }
                        if ui.button("ℹ️").clicked() {
                            self.show_about = !self.show_about;
                        }
                    });
                }
            });
        });

        // --- Bottom Bar (messages + status) ---
        if self.git.is_open() {
            egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Loading indicator when operations are in progress
                    if self.is_busy() {
                        ui.label(
                            egui::RichText::new(format!("⏳ {}...", self.current_operation()))
                                .color(egui::Color32::YELLOW)
                                .size(13.0),
                        );
                    }

                    if !self.error_message.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.error_message)
                                .color(egui::Color32::RED),
                        );
                        if ui.button("x").clicked() {
                            self.error_message.clear();
                        }
                    }
                    if !self.success_message.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.success_message)
                                .color(egui::Color32::GREEN),
                        );
                        if ui.button("x").clicked() {
                            self.success_message.clear();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let elapsed = self.last_refresh.elapsed().as_secs();
                        ui.label(format!("Updated {}s ago", elapsed));
                    });
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
                        let path = native_dialog_path();
                        if let Some(p) = path {
                            self.open_repo(&p);
                        }
                    }
                    ui.add_space(10.0);
                    ui.label("Or drag & drop a folder");
                    if ui.button("Clone Repository...").clicked() {
                        self.current_tab = Tab::Remotes;
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
                    (Tab::Log, "📜 Log"),
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
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                        ui.add_space(8.0);
                        ui.hyperlink("https://github.com/JohnXu22786/GitManager");
                        ui.add_space(8.0);
                        ui.label("A dedicated Git branch & worktree manager.");
                        ui.add_space(4.0);
                        ui.label("Built with Rust + egui + libgit2");
                        ui.add_space(12.0);
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }

        // Keep repainting while operations are in progress
        if self.is_busy() {
            ctx.request_repaint();
        }
    }
}

fn native_dialog_path() -> Option<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.FolderBrowserDialog
$f.Description = "Select a Git repository"
$f.ShowNewFolderButton = $false
$result = $f.ShowDialog()
if ($result -eq 'OK') { Write-Output $f.SelectedPath }
"#;

    let output = Command::new("powershell")
        .creation_flags(0x08000000)
        .arg("-NoProfile")
        .arg("-Command")
        .arg(script)
        .output()
        .ok()?;

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(path) }
}
