use crate::app::App;
use crate::git_ops::BranchInfo;
use crate::git_ops::GitOperation;
use crate::ui::{column_cell, column_header, column_header_static};
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Branches").heading()).truncate()).on_hover_text("Branches");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::add_enabled_ellipsis(ui, !app.is_busy(), "🔄 Refresh").clicked() {
                app.refresh_all();
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.text_edit_singleline(&mut app.branch_filter);
    });

    ui.separator();

    let filter = app.branch_filter.to_lowercase();
    let branches = app.branches.clone();

    let locals: Vec<BranchInfo> = branches.iter()
        .filter(|b| !b.is_remote && (filter.is_empty() || b.name.to_lowercase().contains(&filter)))
        .cloned()
        .collect();
    let remotes: Vec<BranchInfo> = branches.iter()
        .filter(|b| b.is_remote && (filter.is_empty() || b.name.to_lowercase().contains(&filter)))
        .cloned()
        .collect();

    // ── Column header row (left-to-right: Name | Commit, Actions) ─
    ui.horizontal(|ui| {
        let cw = &mut app.column_widths;
        let avail = ui.available_width();
        // Reserve 50px for separator + "Actions" label
        let reserved = 50.0;
        let max_cols = (avail - reserved).max(120.0);

        // Only Name column is draggable (divider between Name and Commit).
        // Commit fills remaining width automatically.
        let mut name_w = cw.get("branch_name", 280.0);
        name_w = name_w.clamp(60.0, max_cols - 60.0);
        let commit_w = max_cols - name_w;

        column_header(ui, "Name", &mut name_w, 60.0, max_cols - 60.0, "branch_name_hdr");
        cw.set("branch_name", name_w);
        column_header_static(ui, "Last Commit", commit_w);

        // Actions (rightmost)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new("Actions").strong()));
        });
    });

    ui.separator();

    // ── Content rows ─────────────────────────────────────────────────
    // Limit to ~2/3 of available height so create/merge/rename sections
    // stay visible and the outer ScrollArea in app.rs rarely triggers.
    let max_list_height = (ui.available_height() * 0.67).max(150.0);
    egui::ScrollArea::vertical()
        .id_salt("branch_list")
        .max_height(max_list_height)
        .show(ui, |ui| {
        ui.label(egui::RichText::new("Local Branches").strong());
        for branch in &locals {
            show_branch_row(app, ui, ctx, branch);
        }

        if !remotes.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.label(egui::RichText::new("Remote Branches").strong());
            for branch in &remotes {
                show_branch_row(app, ui, ctx, branch);
            }
        }
    });

    // ── Create / Merge / Rename sections (unchanged) ────────────────
    ui.add_space(10.0);
    ui.separator();
    ui.heading("Create Branch");
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut app.new_branch_name);
    });
    ui.horizontal(|ui| {
        ui.label("Base:");
        ui.text_edit_singleline(&mut app.new_branch_base);
        ui.label("(leave empty for current HEAD)");
    });
    let busy = app.is_busy();
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Create Branch").clicked() {
        let name = app.new_branch_name.trim().to_string();
        if name.is_empty() {
            app.show_error("Branch name required".into());
        } else {
            let base = if app.new_branch_base.trim().is_empty() {
                None
            } else {
                Some(app.new_branch_base.trim().to_string())
            };
            app.start_operation(ctx, &format!("Create branch '{}'", name), GitOperation::CreateBranch { name, base });
            app.new_branch_name.clear();
            app.new_branch_base.clear();
        }
    }

    ui.separator();
    ui.heading("Merge Branch");
    ui.horizontal(|ui| {
        ui.label("From:");
        ui.text_edit_singleline(&mut app.merge_branch_name);
    });
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Merge").clicked() {
        let name = app.merge_branch_name.trim().to_string();
        if name.is_empty() {
            app.show_error("Branch name required".into());
        } else {
            app.start_operation(ctx, &format!("Merge '{}'", name), GitOperation::MergeBranch(name));
            app.merge_branch_name.clear();
        }
    }

    ui.separator();
    ui.heading("Rename Branch");
    ui.horizontal(|ui| {
        ui.label("From:");
        ui.text_edit_singleline(&mut app.rename_branch_old);
    });
    ui.horizontal(|ui| {
        ui.label("To:");
        ui.text_edit_singleline(&mut app.rename_branch_new);
    });
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Rename").clicked() {
        let old = app.rename_branch_old.trim().to_string();
        let new = app.rename_branch_new.trim().to_string();
        if old.is_empty() || new.is_empty() {
            app.show_error("Both names required".into());
        } else {
            app.start_operation(ctx, &format!("Rename '{}'", old), GitOperation::RenameBranch { old, new: new.clone() });
            app.rename_branch_old.clear();
            app.rename_branch_new.clear();
        }
    }
}

fn show_branch_row(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, branch: &BranchInfo) {
    let dark = ctx.style().visuals.dark_mode;
    let name = branch.name.clone();
    let is_remote = branch.is_remote;
    let is_head = branch.is_head;
    let busy = app.is_busy();

    let name_color = if is_head {
        App::adaptive_green(dark)
    } else {
        ui.style().visuals.text_color()
    };

    // Build name display text with icon merged in
    let icon_prefix = if is_head { "▶ " } else { "  " };
    let name_display = if branch.is_head && !is_remote {
        if let Some(upstream) = &branch.upstream {
            let tracking = if branch.ahead > 0 || branch.behind > 0 {
                format!(" (↑{} ↓{})", branch.ahead, branch.behind)
            } else {
                String::new()
            };
            format!("{}{} → {}{}", icon_prefix, name, upstream, tracking)
        } else {
            format!("{}{}", icon_prefix, name)
        }
    } else {
        format!("{}{}", icon_prefix, name)
    };

    let commit_text: Option<String> = branch.last_commit.as_ref()
        .filter(|m| !m.is_empty())
        .cloned();

    // Name column is draggable; Commit fills remaining space.
    // Reserve 39px for sep (4px) + "…" menu button (~35px).
    let avail = ui.available_width();
    let max_cols = (avail - 39.0).max(120.0);
    let mut name_w = app.column_widths.get("branch_name", 280.0);
    name_w = name_w.clamp(60.0, max_cols - 60.0);
    let commit_w = max_cols - name_w;

    // Left-to-right flow: Name, Commit, "…" menu
    ui.horizontal(|ui| {
        column_cell(ui, name_w, &name_display, name_color);

        column_cell(ui, commit_w, commit_text.as_deref().unwrap_or(""), egui::Color32::GRAY);

        if !is_head || (!is_head && !is_remote) {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.menu_button("…", |ui| {
                    if !is_head {
                        if ui.add_enabled(!busy, egui::Button::new("Checkout")).clicked() {
                            app.start_operation(ctx, &format!("Checkout '{}'", name), GitOperation::CheckoutBranch(name.clone()));
                            ui.close_menu();
                        }
                    }
                    if !is_head && !is_remote {
                        if ui.add_enabled(!busy, egui::Button::new("Copy")).clicked() {
                            ui.ctx().copy_text(name.clone());
                            app.show_success(format!("Copied {}", name));
                            ui.close_menu();
                        }
                        if ui.add_enabled(!busy, egui::Button::new("Delete")).clicked() {
                            app.start_operation(ctx, &format!("Delete '{}'", name), GitOperation::DeleteBranch { name: name.clone(), force: false });
                            ui.close_menu();
                        }
                        if ui.add_enabled(!busy, egui::Button::new("Force Del")).clicked() {
                            app.start_operation(ctx, &format!("Force delete '{}'", name), GitOperation::DeleteBranch { name: name.clone(), force: true });
                            ui.close_menu();
                        }
                    }
                });
            });
        }
    });
}
