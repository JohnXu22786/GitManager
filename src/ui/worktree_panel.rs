use crate::app::{App, FormSubmission};
use crate::git_ops::GitOperation;
use crate::git_ops::WorktreeInfo;
use crate::ui::{column_cell, column_header, column_header_static};
use eframe::egui;

const WORKTREE_ACTIONS_WIDTH: f32 = 90.0;
const WORKTREE_STATUS_MIN_WIDTH: f32 = 88.0;
const WORKTREE_STATUS_WIDTH: f32 = 170.0;

fn worktree_status_column_width(available_width: f32) -> f32 {
    let width = available_width - WORKTREE_ACTIONS_WIDTH - 220.0;
    if width < WORKTREE_STATUS_MIN_WIDTH {
        0.0
    } else {
        width.min(WORKTREE_STATUS_WIDTH)
    }
}

fn worktree_status_label(worktree: &WorktreeInfo, dark: bool) -> (String, egui::Color32) {
    let status = &worktree.status;
    if let Some(error) = status.inspection_error.as_deref() {
        return (
            format!("Status unavailable: {}", error),
            App::adaptive_red(dark),
        );
    }
    if status.directory_missing {
        return ("Directory missing".into(), App::adaptive_yellow(dark));
    }

    let mut parts = vec![if status.change_path_count == 0 {
        "Clean".to_string()
    } else {
        format!("{} affected", status.change_path_count)
    }];
    if status.staged_changes > 0 {
        parts.push(format!("{} staged", status.staged_changes));
    }
    if status.unstaged_changes > 0 {
        parts.push(format!("{} modified", status.unstaged_changes));
    }
    if status.untracked_paths > 0 {
        parts.push(format!("{} untracked", status.untracked_paths));
    }
    if status.ignored_paths > 0 {
        parts.push(format!("{} ignored", status.ignored_paths));
    }
    if status.conflicted_paths > 0 {
        parts.push(format!("{} conflicts", status.conflicted_paths));
    }
    match (status.upstream.as_deref(), status.ahead, status.behind) {
        (Some(upstream), Some(ahead), Some(behind)) => {
            if ahead > 0 {
                parts.push(format!("{} ahead of {}", ahead, upstream));
            }
            if behind > 0 {
                parts.push(format!("{} behind {}", behind, upstream));
            }
            if ahead == 0 && behind == 0 {
                parts.push(format!("Up to date with {}", upstream));
            }
        }
        (Some(_), _, _) => parts.push("Tracking unknown".into()),
        (None, _, _) if worktree.branch.is_some() => parts.push("No upstream".into()),
        _ => {}
    }
    if !worktree.is_main {
        match status.merged_into_main {
            Some(true) => parts.push("Merged to main".into()),
            Some(false) => parts.push("Not merged".into()),
            None => parts.push("Merge status unknown".into()),
        }
    }
    if status.locked {
        parts.push("Locked".into());
    }

    let needs_attention = status.has_removal_blockers()
        || status.ahead.is_some_and(|ahead| ahead > 0)
        || status.behind.is_some_and(|behind| behind > 0)
        || status.merged_into_main == Some(false)
        || (!worktree.is_main && status.merged_into_main.is_none())
        || (status.upstream.is_none() && worktree.branch.is_some())
        || (status.upstream.is_some() && (status.ahead.is_none() || status.behind.is_none()));
    let color = if needs_attention {
        App::adaptive_yellow(dark)
    } else {
        App::adaptive_green(dark)
    };
    (parts.join(" · "), color)
}

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Worktrees").heading()).truncate()).on_hover_text("Worktrees");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.is_busy();
            if crate::ui::add_enabled_ellipsis(ui, !busy, "🔄 Refresh").clicked() {
                app.refresh_all(ctx);
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Prune").clicked() {
                app.start_operation(ctx, "Pruning stale worktrees", GitOperation::PruneWorktrees);
            }
        });
    });

    ui.separator();

    let worktrees = app.worktrees.clone();
    let main_wts: Vec<WorktreeInfo> = worktrees.iter().filter(|w| w.is_main).cloned().collect();
    let linked_wts: Vec<WorktreeInfo> = worktrees.iter().filter(|w| !w.is_main).cloned().collect();

    // ── Column header row (left-to-right: Path | Branch/SHA | Status | Actions) ─
    ui.horizontal(|ui| {
        let cw = &mut app.column_widths;
        let avail = ui.available_width();
        let status_w = worktree_status_column_width(avail);
        let reserved = WORKTREE_ACTIONS_WIDTH + status_w;
        let max_cols = (avail - reserved).max(120.0);

        // Only Path column is draggable; Branch/SHA fills the remaining width.
        let mut path_w = cw.get("worktree_path", 280.0);
        path_w = path_w.clamp(60.0, max_cols - 60.0);
        let bs_w = max_cols - path_w;

        column_header(ui, "Path", &mut path_w, 60.0, max_cols - 60.0, "wt_path_hdr");
        cw.set("worktree_path", path_w);
        column_header_static(ui, "Branch/SHA", bs_w);
        if status_w > 0.0 {
            column_header_static(ui, "Status", status_w);
        }

        // Actions (rightmost)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new("Actions").strong()));
        });
    });

    ui.separator();

    // ── Content ──────────────────────────────────────────────────────
    let max_list_height = (ui.available_height() * 0.67).max(150.0);

    // Main Worktree (scrollable)
    if !main_wts.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt("wt_main_list")
            .max_height(max_list_height)
            .show(ui, |ui| {
            ui.label(egui::RichText::new("Main Worktree").strong());
            for wt in &main_wts {
                show_worktree_row(app, ui, ctx, wt);
            }
        });
    }

    // Separator between Main and Linked (outside ScrollArea)
    if !linked_wts.is_empty() {
        if !main_wts.is_empty() {
            ui.add_space(10.0);
        }
        ui.separator();
    }

    // Linked Worktrees (scrollable)
    if !linked_wts.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt("wt_linked_list")
            .max_height(max_list_height)
            .show(ui, |ui| {
            ui.label(egui::RichText::new("Linked Worktrees").strong());
            for wt in &linked_wts {
                show_worktree_row(app, ui, ctx, wt);
            }
        });
    }

    // Separator after Linked / before Add Worktree (outside ScrollArea)
    if !linked_wts.is_empty() || !main_wts.is_empty() {
        ui.add_space(10.0);
        ui.separator();
    }
    ui.heading("Add Worktree");

    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut app.new_worktree_name);
    });
    egui::CollapsingHeader::new("Advanced")
        .id_salt("worktree_creation_advanced")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut app.new_worktree_path);
                ui.label("(leave empty for default)");
            });
        });
    ui.horizontal(|ui| {
        ui.label("Branch:");
        ui.text_edit_singleline(&mut app.new_worktree_branch);
        ui.checkbox(&mut app.new_worktree_create_branch, "Create new branch");
    });

    let busy = app.is_busy();
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Add Worktree").clicked() {
        let name = app.new_worktree_name.trim().to_string();
        if name.is_empty() {
            app.show_error("Worktree name required".into());
        } else {
            let path = if app.new_worktree_path.trim().is_empty() {
                let Some(repo_path) = app.git.path() else {
                    app.show_error(
                        "Cannot determine the repository path for a default worktree".into(),
                    );
                    return;
                };
                let Some(path) = default_worktree_path(repo_path, &name) else {
                    app.show_error(
                        "Cannot choose a default worktree path because the repository has no parent directory"
                            .into(),
                    );
                    return;
                };
                path
            } else {
                std::path::PathBuf::from(app.new_worktree_path.trim())
            };

            let branch = if app.new_worktree_branch.trim().is_empty() {
                None
            } else {
                Some(app.new_worktree_branch.trim().to_string())
            };

            let form_submission = FormSubmission::CreateWorktree {
                name: app.new_worktree_name.clone(),
                path: app.new_worktree_path.clone(),
                branch: app.new_worktree_branch.clone(),
                create_branch: app.new_worktree_create_branch,
            };
            app.start_operation_with_form_submission(
                ctx,
                &format!("Create worktree '{}'", name),
                GitOperation::CreateWorktree {
                    name,
                    path,
                    branch,
                    new_branch: app.new_worktree_create_branch,
                },
                form_submission,
            );
        }
    }
}

fn default_worktree_path(
    repo_path: &std::path::Path,
    name: &str,
) -> Option<std::path::PathBuf> {
    repo_path.parent().map(|parent| parent.join(name))
}

fn show_worktree_row(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, wt: &WorktreeInfo) {
    let wt_path = wt.path.clone();
    let busy = app.is_busy();
    let icon = if wt.is_main { "★ " } else { "○ " };

    let branch_display = wt.branch.as_deref().unwrap_or("detached");
    let sha_short = wt.sha.get(..7).unwrap_or(&wt.sha);
    let path_display = wt.path.to_string_lossy().to_string();
    let avail = ui.available_width();
    let status_w = worktree_status_column_width(avail);
    let reserved = WORKTREE_ACTIONS_WIDTH + status_w;
    let is_current = app
        .git
        .path()
        .is_some_and(|current_path| current_path == wt_path.as_path());
    let max_cols = (avail - reserved).max(120.0);
    let mut path_w = app.column_widths.get("worktree_path", 280.0);
    path_w = path_w.clamp(60.0, max_cols - 60.0);
    let bs_w = max_cols - path_w;
    let (status_text, status_color) = worktree_status_label(wt, ui.style().visuals.dark_mode);
    let branch_sha_text = if status_w > 0.0 {
        format!("{}{} [{}]", icon, branch_display, sha_short)
    } else {
        format!(
            "{}{} [{}] · {}",
            icon, branch_display, sha_short, status_text
        )
    };

    // Left-to-right flow: Path, Branch/SHA, Status, Actions.
    ui.horizontal(|ui| {
        column_cell(ui, path_w, &path_display, egui::Color32::GRAY);

        column_cell(ui, bs_w, &branch_sha_text, ui.style().visuals.text_color());
        if status_w > 0.0 {
            column_cell(ui, status_w, &status_text, status_color);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            if !wt.is_main {
                ui.menu_button("…", |ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new("Remove…"))
                        .clicked()
                    {
                        app.preview_worktree_cleanup(ctx, wt, false);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Force Remove…"))
                        .clicked()
                    {
                        app.preview_worktree_cleanup(ctx, wt, true);
                        ui.close_menu();
                    }
                });
            }
            if ui
                .add_enabled(!busy && !is_current, egui::Button::new("Open"))
                .clicked()
            {
                open_worktree(app, &wt_path);
            }
        });
    });
}

fn open_worktree(app: &mut App, path: &std::path::Path) {
    app.open_repo_path(path);
}

#[cfg(test)]
mod tests {
    use super::{
        default_worktree_path, open_worktree, worktree_status_column_width, worktree_status_label,
    };
    use crate::app::App;
    use crate::git_ops::{WorktreeInfo, WorktreeStatusSummary};
    use crate::recent::RecentRepos;
    use std::path::{Path, PathBuf};

    #[test]
    fn status_column_hides_when_the_window_is_too_narrow() {
        assert_eq!(worktree_status_column_width(380.0), 0.0);
        assert_eq!(worktree_status_column_width(398.0), 88.0);
        assert_eq!(worktree_status_column_width(800.0), 170.0);
    }

    #[test]
    fn worktree_status_label_surfaces_changes_and_tracking_state() {
        let mut worktree = WorktreeInfo {
            path: PathBuf::from("/repo/feature"),
            branch: Some("feature".into()),
            sha: "1234567890".into(),
            head_snapshot: None,
            is_main: false,
            git_link_identity: None,
            status: WorktreeStatusSummary {
                staged_changes: 1,
                untracked_paths: 1,
                change_path_count: 2,
                upstream: Some("origin/feature".into()),
                ahead: Some(2),
                behind: Some(1),
                merged_into_main: Some(false),
                ..WorktreeStatusSummary::default()
            },
        };

        let (label, color) = worktree_status_label(&worktree, false);
        assert!(label.contains("2 affected"));
        assert!(label.contains("1 staged"));
        assert!(label.contains("1 untracked"));
        assert!(label.contains("2 ahead"));
        assert!(label.contains("1 behind"));
        assert!(label.contains("Not merged"));
        assert_eq!(color, App::adaptive_yellow(false));

        worktree.status = WorktreeStatusSummary {
            inspection_error: Some("status unavailable".into()),
            ..WorktreeStatusSummary::default()
        };
        let (label, color) = worktree_status_label(&worktree, false);
        assert!(label.contains("Status unavailable"));
        assert_eq!(color, App::adaptive_red(false));
    }

    #[test]
    fn root_repository_has_no_default_worktree_path() {
        #[cfg(windows)]
        let root = Path::new(r"C:\");
        #[cfg(not(windows))]
        let root = Path::new("/");

        assert_eq!(default_worktree_path(root, "feature"), None);
    }

    #[test]
    fn open_action_switches_to_worktree_repository() {
        let worktree_dir = tempfile::tempdir().expect("worktree directory");
        drop(git2::Repository::init(worktree_dir.path()).expect("initialize worktree repository"));
        let recent_dir = tempfile::tempdir().expect("recent repositories directory");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_dir.path().join("recent.json"));

        open_worktree(&mut app, worktree_dir.path());

        assert_eq!(app.repo_path, worktree_dir.path().to_str().expect("UTF-8 path"));
        assert_eq!(app.git.path(), Some(worktree_dir.path()));
        let ctx = eframe::egui::Context::default();
        for _ in 0..200 {
            app.process_pending_ops(&ctx);
            if !app.is_busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.is_busy(), "repository refresh should finish");
    }

    #[test]
    fn default_worktree_path_is_sibling_of_repository() {
        let repo_path = PathBuf::from("parent").join("repository");

        assert_eq!(
            default_worktree_path(&repo_path, "feature"),
            Some(PathBuf::from("parent").join("feature"))
        );
    }
}
