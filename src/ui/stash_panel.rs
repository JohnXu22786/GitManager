use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!app.is_busy(), egui::Button::new("🔄 Refresh")).clicked() {
                app.refresh_all();
            }
            ui.add(egui::Label::new(egui::RichText::new("Stash").heading()).truncate()).on_hover_text("Stash");
        });
    });

    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Message:");
        ui.text_edit_singleline(&mut app.stash_message);
    });
    let busy = app.is_busy();
    if ui.add_enabled(!busy, egui::Button::new("Stash All")).clicked() {
        let msg = if app.stash_message.trim().is_empty() {
            None
        } else {
            Some(app.stash_message.trim().to_string())
        };
        app.start_operation(ctx, "Stashing all", GitOperation::StashAll(msg));
        app.stash_message.clear();
    }

    ui.separator();

    if ui.add_enabled(!busy, egui::Button::new("Pop Stash")).clicked() {
        app.start_operation(ctx, "Popping stash", GitOperation::StashPop);
    }
    if ui.add_enabled(!busy, egui::Button::new("Apply Stash")).clicked() {
        app.start_operation(ctx, "Applying stash", GitOperation::StashApply(0));
    }

    ui.separator();

    let stashes = app.stashes.clone();
    if !stashes.is_empty() {
        ui.label(egui::RichText::new(format!("Stash List ({} entries)", stashes.len())).strong());
        for stash in &stashes {
            let index = stash.index;
            ui.horizontal(|ui| {
                let busy = app.is_busy();
                ui.label(
                    egui::RichText::new(format!("stash@{{{}}}", stash.index))
                        .color(egui::Color32::from_rgb(200, 150, 100))
                        .monospace(),
                );
                // Buttons on the right edge, message text truncates in between
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Drop")).clicked() {
                        app.start_operation(ctx, &format!("Drop stash@{{{}}}", index), GitOperation::StashDrop(index));
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Apply")).clicked() {
                        app.start_operation(ctx, &format!("Apply stash@{{{}}}", index), GitOperation::StashApply(index));
                    }
                    // Timestamp (leftmost text)
                    let time_clone = stash.time.clone();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&stash.time)
                                .color(egui::Color32::GRAY)
                                .text_style(egui::TextStyle::Small),
                        )
                        .truncate(),
                    )
                    .on_hover_text(time_clone);
                    // Message (rightmost text, closest to buttons)
                    let msg_clone = stash.message.clone();
                    ui.add(
                        egui::Label::new(&stash.message)
                            .truncate(),
                    )
                    .on_hover_text(msg_clone);
                });
            });
        }
    } else {
        ui.label("No stashes");
    }
}
