use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    ui.horizontal(|ui| {
        ui.heading("Stash");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::add_enabled_ellipsis(ui, !app.is_busy(), "🔄 Refresh").clicked() {
                app.refresh_all();
            }
        });
    });

    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Message:");
        ui.text_edit_singleline(&mut app.stash_message);
    });
    let busy = app.is_busy();
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Stash All").clicked() {
        let msg = if app.stash_message.trim().is_empty() {
            None
        } else {
            Some(app.stash_message.trim().to_string())
        };
        app.start_operation(ctx, "Stashing all", GitOperation::StashAll(msg));
        app.stash_message.clear();
    }

    ui.separator();

    if crate::ui::add_enabled_ellipsis(ui, !busy, "Pop Stash").clicked() {
        app.start_operation(ctx, "Popping stash", GitOperation::StashPop);
    }
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Apply Stash").clicked() {
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
                        .color(if dark {
                            egui::Color32::from_rgb(240, 190, 120)
                        } else {
                            egui::Color32::from_rgb(180, 120, 50)
                        })
                        .monospace(),
                );
                ui.label(&stash.message);
                ui.label(
                    egui::RichText::new(&stash.time)
                        .color(egui::Color32::GRAY)
                        .text_style(egui::TextStyle::Small),
                );
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Drop").clicked() {
                    app.start_operation(ctx, &format!("Drop stash@{{{}}}", index), GitOperation::StashDrop(index));
                }
                if crate::ui::add_enabled_ellipsis(ui, !busy, "Apply").clicked() {
                    // FIX: Pass the specific stash index instead of always applying index 0
                    app.start_operation(ctx, &format!("Apply stash@{{{}}}", index), GitOperation::StashApply(index));
                }
            });
        }
    } else {
        ui.label("No stashes");
    }
}
