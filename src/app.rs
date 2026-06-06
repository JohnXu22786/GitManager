use crate::git_ops::*;
use crate::recent::RecentRepos;
use eframe::egui;
use std::path::Path;

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
    pub refreshing: bool,

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
    pub recent_repos: RecentRepos,
}

impl App {
    pub fn new() -> Self {
        Self {
            git: GitRepo::new(),
            current_tab: Tab::Status,
            repo_path: String::new(),
            error_message: String::new(),
            success_message: String::new(),
            refreshing: false,

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
            recent_repos: RecentRepos::load(),
        }
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

    pub fn refresh_all(&mut self) {
        if !self.git.is_open() {
            return;
        }
        self.refreshing = true;
        self.error_message.clear();
        self.success_message.clear();

        self.status_entries = self.git.get_status().unwrap_or_default();
        self.branches = self.git.branches().unwrap_or_default();
        self.worktrees = self.git.worktrees().unwrap_or_default();
        self.commits = self.git.log(100).unwrap_or_default();
        self.stashes = self.git.stash_list().unwrap_or_default();
        self.remote_list = self.git.remotes().unwrap_or_default();

        self.last_refresh = std::time::Instant::now();
        self.refreshing = false;
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
        let dark = ctx.style().visuals.dark_mode;
        if dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("📂");
                if ui.button("Open Repo...").clicked() {
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
                            .size(12.0),
                    );
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
                        if ui.button("🔄").clicked() {
                            self.refresh_all();
                        }
                    });
                }
            });
        });

        if self.git.is_open() {
            egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.git.is_open() {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("Git Manager");
                    ui.label("Open a Git repository to get started.");
                    ui.add_space(20.0);
                    if ui.button("📂 Open Repository").clicked() {
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

            match self.current_tab {
                Tab::Status => crate::ui::status_panel::show(self, ui),
                Tab::Branches => crate::ui::branch_panel::show(self, ui),
                Tab::Worktrees => crate::ui::worktree_panel::show(self, ui),
                Tab::Log => crate::ui::log_panel::show(self, ui),
                Tab::Stash => crate::ui::stash_panel::show(self, ui),
                Tab::Remotes => crate::ui::remote_panel::show(self, ui),
            }
        });

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
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }

        if self.refreshing {
            ctx.request_repaint();
        }
    }
}
