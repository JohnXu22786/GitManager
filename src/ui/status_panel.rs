use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.heading("Changes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.is_busy();
            if ui.add_enabled(!busy, egui::Button::new("Stage All")).clicked() {
                app.start_operation(ctx, "Staging all", GitOperation::StageAll);
            }
            if ui.add_enabled(!busy, egui::Button::new("Unstage All")).clicked() {
                app.start_operation(ctx, "Unstaging all", GitOperation::UnstageAll);
            }
            if ui.add_enabled(!busy, egui::Button::new("Discard All")).clicked() {
                app.start_operation(ctx, "Discarding all", GitOperation::RestoreAll);
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
                let busy = app.is_busy();
                let color = crate::app::App::status_color_by_type(entry.status);
                ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());
                if ui.add_enabled(!busy, egui::Button::new("Unstage")).clicked() {
                    app.start_operation(ctx, &format!("Unstage {}", path), GitOperation::UnstageFile(path.clone()));
                }
                if ui.add_enabled(!busy, egui::Button::new("Diff")).clicked() {
                    app.start_operation(ctx, &format!("Diff {}", path), GitOperation::GetDiff { path: path.clone(), staged: true });
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
                let busy = app.is_busy();
                let color = crate::app::App::status_color_by_type(entry.status);
                ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());

                if entry.status != 'D' && entry.status != '?' && entry.status != '!' {
                    if ui.add_enabled(!busy, egui::Button::new("Stage")).clicked() {
                        app.start_operation(ctx, &format!("Stage {}", path), GitOperation::StageFile(path.clone()));
                    }
                }
                if entry.status != '?' && entry.status != '!' {
                    if ui.add_enabled(!busy, egui::Button::new("Discard")).clicked() {
                        app.start_operation(ctx, &format!("Restore {}", path), GitOperation::RestoreFile(path.clone()));
                    }
                }
                if ui.add_enabled(!busy, egui::Button::new("Diff")).clicked() {
                    app.start_operation(ctx, &format!("Diff {}", path), GitOperation::GetDiff { path: path.clone(), staged: false });
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

    let busy = app.is_busy();
    if ui.add_enabled(!busy, egui::Button::new("Commit")).clicked() {
        if app.commit_msg.trim().is_empty() {
            app.show_error("Commit message cannot be empty".into());
        } else {
            let msg = app.commit_msg.trim().to_string();
            let amend = app.commit_amend;
            app.start_operation(ctx, "Committing", GitOperation::Commit { message: msg, amend });
            app.commit_msg.clear();
            app.commit_amend = false;
        }
    }
    if ui.add_enabled(!busy, egui::Button::new("Uncommit")).clicked() {
        app.start_operation(ctx, "Uncommitting", GitOperation::Uncommit);
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
