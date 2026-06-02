use crate::app::App;
use eframe::egui;
use crate::git_ops::BranchInfo;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Branches");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🔄 Refresh").clicked() {
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
            show_branch_row(app, ui, branch);
        }

        if !remotes.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.label(egui::RichText::new("Remote Branches").strong());
            for branch in &remotes {
                show_branch_row(app, ui, branch);
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
    if ui.button("Create Branch").clicked() {
        let name = app.new_branch_name.trim().to_string();
        if name.is_empty() {
            app.show_error("Branch name required".into());
        } else {
            let base = if app.new_branch_base.trim().is_empty() {
                None
            } else {
                Some(app.new_branch_base.trim().to_string())
            };
            match app.git.create_branch(&name, base.as_deref()) {
                Ok(()) => {
                    app.show_success(format!("Created branch '{}'", name));
                    app.new_branch_name.clear();
                    app.new_branch_base.clear();
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
        }
    }

    ui.separator();
    ui.heading("Merge Branch");
    ui.horizontal(|ui| {
        ui.label("From:");
        ui.text_edit_singleline(&mut app.merge_branch_name);
    });
    if ui.button("Merge").clicked() {
        let name = app.merge_branch_name.trim().to_string();
        if name.is_empty() {
            app.show_error("Branch name required".into());
        } else {
            match app.git.merge_branch(&name) {
                Ok(msg) => {
                    app.show_success(msg);
                    app.merge_branch_name.clear();
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
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
    if ui.button("Rename").clicked() {
        let old = app.rename_branch_old.trim().to_string();
        let new = app.rename_branch_new.trim().to_string();
        if old.is_empty() || new.is_empty() {
            app.show_error("Both names required".into());
        } else {
            match app.git.rename_branch(&old, &new) {
                Ok(()) => {
                    app.show_success(format!("Renamed '{}' -> '{}'", old, new));
                    app.rename_branch_old.clear();
                    app.rename_branch_new.clear();
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
        }
    }
}

fn show_branch_row(app: &mut App, ui: &mut egui::Ui, branch: &BranchInfo) {
    let name = branch.name.clone();
    let is_remote = branch.is_remote;
    let is_head = branch.is_head;

    ui.horizontal(|ui| {
        let icon = if is_head { "▶" } else { " " };
        let name_color = if is_head {
            egui::Color32::GREEN
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
                        .size(12.0),
                );
            }
        }

        if let Some(msg) = &branch.last_commit {
            if !msg.is_empty() {
                ui.label(
                    egui::RichText::new(msg)
                        .color(egui::Color32::GRAY)
                        .size(11.0),
                );
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !is_head && !is_remote {
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(name.clone());
                    app.show_success(format!("Copied {}", name));
                }
                if ui.button("Delete").clicked() {
                    match app.git.delete_branch(&name, false) {
                        Ok(()) => {
                            app.show_success(format!("Deleted '{}'", name));
                            app.refresh_all();
                        }
                        Err(e) => app.show_error(e),
                    }
                }
                if ui.button("Force Del").clicked() {
                    match app.git.delete_branch(&name, false) {
                        Ok(()) => {
                            app.show_success(format!("Force deleted '{}'", name));
                            app.refresh_all();
                        }
                        Err(e) => app.show_error(e),
                    }
                }
            }
            if !is_head {
                if ui.button("Checkout").clicked() {
                    match app.git.checkout_branch(&name) {
                        Ok(()) => {
                            app.show_success(format!("Switched to '{}'", name));
                            app.refresh_all();
                        }
                        Err(e) => app.show_error(e),
                    }
                }
            }
        });
    });
}
