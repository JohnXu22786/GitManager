use crate::app::App;
use crate::git_ops::GitOperation;
use crate::git_ops::WorktreeInfo;
use crate::ui::{column_cell, column_header, column_header_static};
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Worktrees").heading()).truncate()).on_hover_text("Worktrees");
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

    // ── Column header row (left-to-right: Path | Branch/SHA, Actions) ─
    ui.horizontal(|ui| {
        let cw = &mut app.column_widths;
        let avail = ui.available_width();
        // Reserve 50px for "Actions" label
        let reserved = 50.0;
        let max_cols = (avail - reserved).max(120.0);

        // Only Path column is draggable (divider between Path and Branch/SHA).
        // Branch/SHA fills remaining width automatically.
        let mut path_w = cw.get("worktree_path", 280.0);
        path_w = path_w.clamp(60.0, max_cols - 60.0);
        let bs_w = max_cols - path_w;

        column_header(ui, "Path", &mut path_w, 60.0, max_cols - 60.0, "wt_path_hdr");
        cw.set("worktree_path", path_w);
        column_header_static(ui, "Branch/SHA", bs_w);

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
    let wt_path = wt.path.clone();
    let busy = app.is_busy();
    let icon = if wt.is_main { "★ " } else { "○ " };

    let branch_display = wt.branch.as_deref().unwrap_or("detached");
    let sha_short = wt.sha.get(..7).unwrap_or(&wt.sha);
    let branch_sha_text = format!("{}{} [{}]", icon, branch_display, sha_short);
    let path_display = wt.path.to_string_lossy().to_string();

    // Path column is draggable; Branch/SHA fills remaining space.
    // For main worktree rows (no actions), reserved=0; for linked, reserve 39px for "…" button.
    let avail = ui.available_width();
    let reserved = if !wt.is_main { 39.0 } else { 0.0 };
    let max_cols = (avail - reserved).max(120.0);
    let mut path_w = app.column_widths.get("worktree_path", 280.0);
    path_w = path_w.clamp(60.0, max_cols - 60.0);
    let bs_w = max_cols - path_w;

    // Left-to-right flow: Path, Branch/SHA, "…" menu
    ui.horizontal(|ui| {
        column_cell(ui, path_w, &path_display, egui::Color32::GRAY);

        column_cell(ui, bs_w, &branch_sha_text, ui.style().visuals.text_color());

        if !wt.is_main {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.menu_button("…", |ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                        app.start_operation(ctx, &format!("Remove {:?}", wt_path), GitOperation::RemoveWorktree { path: wt_path.clone(), force: false });
                        ui.close_menu();
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Force Remove")).clicked() {
                        app.start_operation(ctx, &format!("Force remove {:?}", wt_path), GitOperation::RemoveWorktree { path: wt_path.clone(), force: true });
                        ui.close_menu();
                    }
                });
            });
        }
    });
}
