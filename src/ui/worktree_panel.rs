use crate::app::App;
use crate::git_ops::GitOperation;
use crate::git_ops::WorktreeInfo;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.heading("Worktrees");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.is_busy();
            if crate::ui::add_enabled_ellipsis(ui, !busy, "🔄 Refresh").clicked() {
                app.refresh_all();
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Prune").clicked() {
                let worktrees = app.worktrees.clone();
                for wt in worktrees {
                    if !wt.is_main {
                        app.start_operation(ctx, &format!("Prune {:?}", wt.path), GitOperation::RemoveWorktree { path: wt.path, force: true });
                    }
                }
                app.show_success("Pruning worktrees...".into());
            }
        });
    });

    ui.separator();

    let worktrees = app.worktrees.clone();
    let main_wts: Vec<WorktreeInfo> = worktrees.iter().filter(|w| w.is_main).cloned().collect();
    let linked_wts: Vec<WorktreeInfo> = worktrees.iter().filter(|w| !w.is_main).cloned().collect();

    egui::ScrollArea::vertical().show(ui, |ui| {
        if !main_wts.is_empty() {
            ui.label(egui::RichText::new("Main Worktree").strong());
            for wt in &main_wts {
                show_worktree_row(app, ui, ctx, wt);
            }
        }

        if !linked_wts.is_empty() {
            ui.add_space(5.0);
            ui.separator();
            ui.label(egui::RichText::new("Linked Worktrees").strong());
            for wt in &linked_wts {
                show_worktree_row(app, ui, ctx, wt);
            }
        }
    });

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Add Worktree");

    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut app.new_worktree_name);
    });
    ui.horizontal(|ui| {
        ui.label("Path:");
        ui.text_edit_singleline(&mut app.new_worktree_path);
        ui.label("(leave empty for default)");
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
                let parent = app.git.path().unwrap().parent().unwrap();
                parent.join(&name)
            } else {
                std::path::PathBuf::from(app.new_worktree_path.trim())
            };

            let branch = if app.new_worktree_branch.trim().is_empty() {
                None
            } else {
                Some(app.new_worktree_branch.trim().to_string())
            };

            app.start_operation(
                ctx,
                &format!("Create worktree '{}'", name),
                GitOperation::CreateWorktree {
                    name,
                    path,
                    branch,
                    new_branch: app.new_worktree_create_branch,
                },
            );
            app.new_worktree_name.clear();
            app.new_worktree_path.clear();
            app.new_worktree_branch.clear();
            app.new_worktree_create_branch = false;
        }
    }
}

fn show_worktree_row(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, wt: &WorktreeInfo) {
    let dark = ctx.style().visuals.dark_mode;
    let wt_path = wt.path.clone();
    ui.horizontal(|ui| {
        let icon = if wt.is_main { "★" } else { "○" };
        ui.label(egui::RichText::new(icon).color(App::adaptive_gold(dark)));

        let branch_display = wt.branch.as_deref().unwrap_or("detached");
        let sha_short = wt.sha.get(..7).unwrap_or(&wt.sha);
        ui.label(format!("{} [{}]", branch_display, sha_short));
        ui.label(
            egui::RichText::new(wt.path.to_string_lossy())
                .color(egui::Color32::GRAY)
                .text_style(egui::TextStyle::Small),
        );

        if !wt.is_main {
            let busy = app.is_busy();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Remove").clicked() {
                    app.start_operation(ctx, &format!("Remove {:?}", wt_path), GitOperation::RemoveWorktree { path: wt_path.clone(), force: false });
                }
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Force Remove").clicked() {
                    app.start_operation(ctx, &format!("Force remove {:?}", wt_path), GitOperation::RemoveWorktree { path: wt_path.clone(), force: true });
                }
            });
        }
    });
}
