use crate::app::App;
use crate::git_ops::GitOperation;
use crate::git_ops::WorktreeInfo;
use crate::ui::{column_cell, column_header, column_separator};
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

    // ── Column header row (left-to-right: Path, Branch/SHA, sep, Actions) ─
    ui.horizontal(|ui| {
        let cw = &mut app.column_widths;
        let avail = ui.available_width();
        let max_cols = (avail - 40.0).max(120.0);

        // Path header (resizable, capped)
        let mut path_w = cw.get("worktree_path", 250.0);
        // Branch/SHA header (resizable, capped)
        let mut bs_w = cw.get("worktree_branch_sha", 220.0);
        if path_w + bs_w > max_cols {
            path_w = max_cols - bs_w;
        }
        path_w = path_w.max(60.0);
        bs_w = bs_w.max(60.0);

        column_header(ui, "Path", &mut path_w, 60.0, "wt_path_hdr");
        cw.set("worktree_path", path_w);
        column_header(ui, "Branch/SHA", &mut bs_w, 60.0, "wt_bs_hdr");
        cw.set("worktree_branch_sha", bs_w);

        column_separator(ui);

        // Actions (rightmost)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(egui::RichText::new("Actions").strong()));
        });
    });

    ui.separator();

    // ── Content ──────────────────────────────────────────────────────
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

    // ── Add Worktree section (unchanged) ─────────────────────────────
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
    let wt_path = wt.path.clone();
    let busy = app.is_busy();
    let icon = if wt.is_main { "★ " } else { "○ " };

    // Copy widths before layout (avoids borrow conflict)
    let mut path_w = app.column_widths.get("worktree_path", 250.0);
    let mut bs_w = app.column_widths.get("worktree_branch_sha", 220.0);

    let branch_display = wt.branch.as_deref().unwrap_or("detached");
    let sha_short = wt.sha.get(..7).unwrap_or(&wt.sha);
    let branch_sha_text = format!("{}{} [{}]", icon, branch_display, sha_short);
    let path_display = wt.path.to_string_lossy().to_string();

    // Cap column widths to not overflow past "…" button
    let avail = ui.available_width();
    let sep = if !wt.is_main { 4.0 } else { 0.0 };
    let btn = if !wt.is_main { 35.0 } else { 0.0 };
    let max_cols = (avail - btn - sep).max(120.0);
    if path_w + bs_w > max_cols {
        path_w = max_cols - bs_w;
    }
    path_w = path_w.max(60.0);
    bs_w = bs_w.max(60.0);

    // Left-to-right flow: Path, Branch/SHA, sep, "…" menu
    ui.horizontal(|ui| {
        column_cell(ui, path_w, &path_display, egui::Color32::GRAY);

        column_cell(ui, bs_w, &branch_sha_text, ui.style().visuals.text_color());

        if !wt.is_main {
            column_separator(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
