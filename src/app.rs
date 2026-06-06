use crate::git_ops::*;
use eframe::egui;
use std::path::Path;

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
    pub font_size: f32,
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
            font_size: 14.0,
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

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂").clicked() {
                    let path = native_dialog_path();
                    if let Some(p) = path {
                        self.open_repo(&p);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("ⓘ").clicked() {
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
                        if ui.button("ⓘ").clicked() {
                            self.show_about = !self.show_about;
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
                        ui.separator();
                        ui.label("Font:");
                        ui.add(
                            egui::Slider::new(&mut self.font_size, Self::MIN_FONT_SIZE..=Self::MAX_FONT_SIZE)
                                .show_value(false),
                        );
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

        if self.refreshing {
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
        // Default value should be between min and max
        assert!(app.font_size >= App::MIN_FONT_SIZE);
        assert!(app.font_size <= App::MAX_FONT_SIZE);
    }

    #[test]
    fn test_status_icon_known_values() {
        assert_eq!(App::status_icon('M'), "M");
        assert_eq!(App::status_icon('A'), "A");
        assert_eq!(App::status_icon('D'), "D");
        assert_eq!(App::status_icon('R'), "R");
        assert_eq!(App::status_icon('C'), "C");
        assert_eq!(App::status_icon('?'), "?");
        assert_eq!(App::status_icon('!'), "!");
        assert_eq!(App::status_icon('U'), "U");
    }

    #[test]
    fn test_status_icon_unknown() {
        assert_eq!(App::status_icon('X'), " ");
        assert_eq!(App::status_icon(' '), " ");
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
    fn test_status_label() {
        assert_eq!(App::status_label(' '), "");
        assert_eq!(App::status_label('+'), "");
        assert_eq!(App::status_label('-'), "del");
        assert_eq!(App::status_label('M'), "mod");
        assert_eq!(App::status_label('?'), "mod");
    }

    #[test]
    fn test_status_color() {
        assert_eq!(App::status_color('+'), egui::Color32::GREEN);
        assert_eq!(App::status_color('-'), egui::Color32::RED);
        assert_eq!(App::status_color('M'), egui::Color32::GRAY);
    }

    #[test]
    fn test_tab_partial_eq() {
        assert_eq!(Tab::Status, Tab::Status);
        assert_eq!(Tab::Log, Tab::Log);
        assert_ne!(Tab::Status, Tab::Branches);
    }

    #[test]
    fn test_tab_clone() {
        let tab = Tab::Worktrees;
        assert_eq!(tab.clone(), Tab::Worktrees);
    }
}
