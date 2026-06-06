use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    ui.horizontal(|ui| {
        ui.heading("Remotes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!app.is_busy(), egui::Button::new("🔄 Refresh")).clicked() {
                app.refresh_all();
            }
        });
    });

    ui.separator();

    if !app.remote_list.is_empty() {
        ui.label(egui::RichText::new("Configured Remotes").strong());
        for remote in &app.remote_list {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&remote.name).color(if dark {
                        egui::Color32::from_rgb(120, 220, 255)
                    } else {
                        egui::Color32::from_rgb(0, 120, 200)
                    }).strong());
                    ui.label(
                        egui::RichText::new(&remote.url)
                            .color(egui::Color32::GRAY)
                            .text_style(egui::TextStyle::Small),
                    );
                });
            });
        }
    } else {
        ui.label("No remotes configured");
    }

    ui.add_space(10.0);
    ui.separator();

    let current_branch = app.git.current_branch().unwrap_or_default();
    let default_remote = app.remote_list.first().map(|r| r.name.clone()).unwrap_or_default();

    if app.push_branch.is_empty() {
        app.push_branch = current_branch.clone();
    }
    if app.remote_name.is_empty() {
        app.remote_name = default_remote.clone();
    }

    let busy = app.is_busy();

    ui.heading("Push");
    ui.horizontal(|ui| {
        ui.label("Remote:");
        ui.text_edit_singleline(&mut app.remote_name);
    });
    ui.horizontal(|ui| {
        ui.label("Branch:");
        ui.text_edit_singleline(&mut app.push_branch);
    });
    ui.checkbox(&mut app.push_force, "Force Push");
    if ui.add_enabled(!busy, egui::Button::new("Push")).clicked() {
        let remote = app.remote_name.trim().to_string();
        let branch = app.push_branch.trim().to_string();
        if remote.is_empty() || branch.is_empty() {
            app.show_error("Remote and branch required".into());
        } else {
            app.start_operation(ctx, &format!("Push to {}/{}", remote, branch), GitOperation::Push {
                remote,
                branch,
                force: app.push_force,
            });
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Pull");
    ui.checkbox(&mut app.pull_rebase, "Rebase instead of merge");
    if ui.add_enabled(!busy, egui::Button::new("Pull")).clicked() {
        let remote = app.remote_name.trim().to_string();
        let branch = app.push_branch.trim().to_string();
        if remote.is_empty() || branch.is_empty() {
            app.show_error("Remote and branch required".into());
        } else {
            app.start_operation(ctx, &format!("Pull from {}/{}", remote, branch), GitOperation::Pull {
                remote,
                branch,
                rebase: app.pull_rebase,
            });
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Fetch");
    if ui.add_enabled(!busy, egui::Button::new("Fetch")).clicked() {
        let remote = app.remote_name.trim().to_string();
        if remote.is_empty() {
            app.show_error("Remote required".into());
        } else {
            app.start_operation(ctx, &format!("Fetch from {}", remote), GitOperation::Fetch(remote));
        }
    }
}
