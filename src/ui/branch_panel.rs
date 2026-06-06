use crate::app::App;
use crate::git_ops::BranchInfo;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.heading("Branches");
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

    egui::ScrollArea::vertical().show(ui, |ui| {
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

    ui.horizontal(|ui| {
        let icon = if is_head { "▶" } else { " " };
        let name_color = if is_head {
            App::adaptive_green(dark)
        } else {
            ui.style().visuals.text_color()
        };

        ui.label(egui::RichText::new(icon).color(name_color).strong());
        ui.label(egui::RichText::new(&branch.name).color(name_color));

        if let Some(upstream) = &branch.upstream {
            if !upstream.is_empty() {
                let tracking = if branch.ahead > 0 || branch.behind > 0 {
                    format!(" (↑{} ↓{})", branch.ahead, branch.behind)
                } else {
                    String::new()
                };
                ui.label(
                    egui::RichText::new(format!("→ {} {}", upstream, tracking))
                        .color(egui::Color32::GRAY)
                        .text_style(egui::TextStyle::Small),
                );
            }
        }

        if let Some(msg) = &branch.last_commit {
            if !msg.is_empty() {
                ui.label(
                    egui::RichText::new(msg)
                        .color(egui::Color32::GRAY)
                        .text_style(egui::TextStyle::Small),
                );
            }
        }

        let busy = app.is_busy();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !is_head && !is_remote {
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Copy").clicked() {
                    ui.ctx().copy_text(name.clone());
                    app.show_success(format!("Copied {}", name));
                }
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Delete").clicked() {
                    app.start_operation(ctx, &format!("Delete '{}'", name), GitOperation::DeleteBranch { name: name.clone(), force: false });
                }
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Force Del").clicked() {
                    // FIX: Pass force=true for Force Del, and try reference deletion
                    app.start_operation(ctx, &format!("Force delete '{}'", name), GitOperation::DeleteBranch { name: name.clone(), force: true });
                }
            }
            if !is_head {
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Checkout").clicked() {
                    app.start_operation(ctx, &format!("Checkout '{}'", name), GitOperation::CheckoutBranch(name));
                }
            }
        });
    });
}
