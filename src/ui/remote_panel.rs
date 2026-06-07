use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::add_enabled_ellipsis(ui, !app.is_busy(), "🔄 Refresh").clicked() {
                app.refresh_all();
            }
            ui.add(egui::Label::new(egui::RichText::new("Remotes").heading()).truncate()).on_hover_text("Remotes");
        });
    });

    ui.separator();

    if !app.remote_list.is_empty() {
        ui.label(egui::RichText::new("Configured Remotes").strong());
        for remote in &app.remote_list {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    let name_clone = remote.name.clone();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&remote.name).color(egui::Color32::from_rgb(100, 200, 255)).strong(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(name_clone);
                    let url_clone = remote.url.clone();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&remote.url)
                                .color(egui::Color32::GRAY)
                                .text_style(egui::TextStyle::Small),
                        )
                        .truncate(),
                    )
                    .on_hover_text(url_clone);

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

    app.push_branch = current_branch.clone();
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
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Push").clicked() {
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
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Pull").clicked() {
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
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Fetch").clicked() {
        let remote = app.remote_name.trim().to_string();
        if remote.is_empty() {
            app.show_error("Remote required".into());
        } else {
            app.start_operation(ctx, &format!("Fetch from {}", remote), GitOperation::Fetch(remote));
        }
    }
}
