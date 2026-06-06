use crate::app::App;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Remotes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🔄 Refresh").clicked() {
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
                    ui.label(egui::RichText::new(&remote.name).color(egui::Color32::from_rgb(100, 200, 255)).strong());
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
    if ui.button("Push").clicked() {
        let remote = app.remote_name.trim().to_string();
        let branch = app.push_branch.trim().to_string();
        if remote.is_empty() || branch.is_empty() {
            app.show_error("Remote and branch required".into());
        } else {
            match app.git.push(&remote, &branch, app.push_force) {
                Ok(progress) => {
                    app.show_success(progress);
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Pull");
    ui.checkbox(&mut app.pull_rebase, "Rebase instead of merge");
    if ui.button("Pull").clicked() {
        let remote = app.remote_name.trim().to_string();
        let branch = app.push_branch.trim().to_string();
        if remote.is_empty() || branch.is_empty() {
            app.show_error("Remote and branch required".into());
        } else {
            match app.git.pull(&remote, &branch, app.pull_rebase) {
                Ok(msg) => {
                    app.show_success(msg);
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Fetch");
    if ui.button("Fetch").clicked() {
        let remote = app.remote_name.trim().to_string();
        if remote.is_empty() {
            app.show_error("Remote required".into());
        } else {
            match app.git.fetch(&remote) {
                Ok(progress) => {
                    app.show_success(progress);
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
        }
    }
}
