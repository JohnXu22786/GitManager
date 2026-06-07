use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Heading row: heading text truncates, buttons stay anchored to right edge
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.is_busy();
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Stage All").clicked() {
                app.start_operation(ctx, "Staging all", GitOperation::StageAll);
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Unstage All").clicked() {
                app.start_operation(ctx, "Unstaging all", GitOperation::UnstageAll);
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Discard All").clicked() {
                app.start_operation(ctx, "Discarding all", GitOperation::RestoreAll);
            }
            ui.add(egui::Label::new(egui::RichText::new("Changes").heading()).truncate()).on_hover_text("Changes");
        });
    });

    ui.separator();

    let staged: Vec<_> = app.status_entries.iter().filter(|e| e.staged).cloned().collect();
    let unstaged: Vec<_> = app.status_entries.iter().filter(|e| !e.staged).cloned().collect();

    if !staged.is_empty() {
        ui.label(egui::RichText::new("Staged").color(App::adaptive_green(dark)).strong());
        for entry in &staged {
            let path = entry.path.clone();
            ui.horizontal(|ui| {
                let busy = app.is_busy();
                let color = crate::app::App::status_color_by_type(entry.status, dark);
                ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());
                // Buttons anchored to right edge, path truncates in between
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Unstage")).clicked() {
                        app.start_operation(ctx, &format!("Unstage {}", path), GitOperation::UnstageFile(path.clone()));
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Diff")).clicked() {
                        app.start_operation(ctx, &format!("Diff {}", path), GitOperation::GetDiff { path: path.clone(), staged: true });
                    }
                    let path_clone = path.clone();
                    ui.add(egui::Label::new(&path).truncate()).on_hover_text(path_clone);
                });
            });
        }
        ui.separator();
    }

    if !unstaged.is_empty() {
        ui.label(egui::RichText::new("Unstaged").color(App::adaptive_yellow(dark)).strong());
        for entry in &unstaged {
            let path = entry.path.clone();
            ui.horizontal(|ui| {
                let busy = app.is_busy();
                let color = crate::app::App::status_color_by_type(entry.status, dark);
                ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                    let path_clone = path.clone();
                    ui.add(egui::Label::new(&path).truncate()).on_hover_text(path_clone);
                });
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
        .id_salt("commit_scroll")
        .show(ui, |ui| {
            ui.add_sized(
                egui::vec2(ui.available_width(), 80.0),
                egui::TextEdit::multiline(commit_msg).hint_text("Commit message"),
            );
        });

    let busy = app.is_busy();
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Commit").clicked() {
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
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Uncommit").clicked() {
        app.start_operation(ctx, "Uncommitting", GitOperation::Uncommit);
    }

    if app.show_diff && !app.diff_content.is_empty() {
        ui.separator();
        ui.heading(format!("Diff: {}", app.diff_path));
        let diff_content = app.diff_content.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &diff_content {
                let color = match line.origin {
                    '+' => if dark {
                        egui::Color32::from_rgb(60, 200, 60)
                    } else {
                        egui::Color32::from_rgb(0, 130, 0)
                    },
                    '-' => if dark {
                        egui::Color32::from_rgb(220, 60, 60)
                    } else {
                        egui::Color32::from_rgb(170, 30, 30)
                    },
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
