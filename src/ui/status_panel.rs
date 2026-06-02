use crate::app::App;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Changes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Stage All").clicked() {
                match app.git.stage_all() {
                    Ok(()) => { app.show_success("Staged all".into()); app.refresh_all(); }
                    Err(e) => app.show_error(e),
                }
            }
            if ui.button("Unstage All").clicked() {
                match app.git.unstage_all() {
                    Ok(()) => { app.show_success("Unstaged all".into()); app.refresh_all(); }
                    Err(e) => app.show_error(e),
                }
            }
            if ui.button("Discard All").clicked() {
                match app.git.restore_all() {
                    Ok(()) => { app.show_success("Restored all".into()); app.refresh_all(); }
                    Err(e) => app.show_error(e),
                }
            }
        });
    });

    ui.separator();

    let staged: Vec<_> = app.status_entries.iter().filter(|e| e.staged).cloned().collect();
    let unstaged: Vec<_> = app.status_entries.iter().filter(|e| !e.staged).cloned().collect();

    if !staged.is_empty() {
        ui.label(egui::RichText::new("Staged").color(egui::Color32::GREEN).strong());
        for entry in &staged {
            let path = entry.path.clone();
            ui.horizontal(|ui| {
                let color = crate::app::App::status_color_by_type(entry.status);
                ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());
                if ui.button("Unstage").clicked() {
                    match app.git.unstage_file(&path) {
                        Ok(()) => { app.show_success(format!("Unstaged {}", path)); app.refresh_all(); }
                        Err(e) => app.show_error(e),
                    }
                }
                if ui.button("Diff").clicked() {
                    app.diff_content = app.git.get_diff(&path, true).unwrap_or_default();
                    app.diff_path = path.clone();
                    app.show_diff = true;
                }
                ui.label(&path);
            });
        }
        ui.separator();
    }

    if !unstaged.is_empty() {
        ui.label(egui::RichText::new("Unstaged").color(egui::Color32::YELLOW).strong());
        for entry in &unstaged {
            let path = entry.path.clone();
            ui.horizontal(|ui| {
                let color = crate::app::App::status_color_by_type(entry.status);
                ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());

                if entry.status != 'D' && entry.status != '?' && entry.status != '!' {
                    if ui.button("Stage").clicked() {
                        match app.git.stage_file(&path) {
                            Ok(()) => { app.show_success(format!("Staged {}", path)); app.refresh_all(); }
                            Err(e) => app.show_error(e),
                        }
                    }
                }
                if entry.status != '?' && entry.status != '!' {
                    if ui.button("Discard").clicked() {
                        match app.git.restore_file(&path) {
                            Ok(()) => { app.show_success(format!("Restored {}", path)); app.refresh_all(); }
                            Err(e) => app.show_error(e),
                        }
                    }
                }
                if ui.button("Diff").clicked() {
                    app.diff_content = app.git.get_diff(&path, false).unwrap_or_default();
                    app.diff_path = path.clone();
                    app.show_diff = true;
                }
                ui.label(&path);
            });
        }
    }

    if staged.is_empty() && unstaged.is_empty() {
        ui.label("No changes - working tree clean");
    }

    ui.separator();
    ui.add_space(10.0);

    ui.heading("Commit");
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.commit_amend, "Amend");
    });

    let commit_msg = &mut app.commit_msg;
    egui::ScrollArea::vertical()
        .id_source("commit_scroll")
        .show(ui, |ui| {
            ui.add_sized(
                egui::vec2(ui.available_width(), 80.0),
                egui::TextEdit::multiline(commit_msg).hint_text("Commit message"),
            );
        });
    if ui.button("Commit").clicked() {
        if app.commit_msg.trim().is_empty() {
            app.show_error("Commit message cannot be empty".into());
        } else {
            match app.git.commit(app.commit_msg.trim(), app.commit_amend) {
                Ok(sha) => {
                    app.show_success(format!("Committed: {}", &sha[..sha.len().min(7)]));
                    app.commit_msg.clear();
                    app.commit_amend = false;
                    app.refresh_all();
                }
                Err(e) => app.show_error(e),
            }
        }
    }
    if ui.button("Uncommit").clicked() {
        match app.git.uncommit() {
            Ok(sha) => {
                app.show_success(format!("Uncommitted to {}", &sha[..sha.len().min(7)]));
                app.refresh_all();
            }
            Err(e) => app.show_error(e),
        }
    }

    if app.show_diff && !app.diff_content.is_empty() {
        ui.separator();
        ui.heading(format!("Diff: {}", app.diff_path));
        let diff_content = app.diff_content.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &diff_content {
                let color = match line.origin {
                    '+' => egui::Color32::from_rgb(40, 180, 40),
                    '-' => egui::Color32::from_rgb(180, 40, 40),
                    _ => egui::Color32::GRAY,
                };
                let prefix = match line.origin {
                    '+' => "+",
                    '-' => "-",
                    ' ' => " ",
                    _ => " ",
                };
                ui.label(
                    egui::RichText::new(format!("{}{}", prefix, line.content.trim_end()))
                        .color(color)
                        .monospace(),
                );
            }
        });
    }
}
