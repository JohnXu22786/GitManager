use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    ui.horizontal(|ui| {
        ui.heading("Commit Log");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!app.is_busy(), egui::Button::new("🔄 Refresh")).clicked() {
                app.refresh_all();
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Search:");
        if ui.text_edit_singleline(&mut app.log_search).changed() {
            let filter = app.log_search.clone();
            app.start_operation(ctx, "Searching commits", GitOperation::LogSearch(filter));
        }
    });

    ui.separator();

    let commits = app.commits.clone();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for commit in &commits {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&commit.short_sha)
                        .color(if dark {
                            egui::Color32::from_rgb(220, 200, 120)
                        } else {
                            egui::Color32::from_rgb(160, 120, 40)
                        })
                        .monospace(),
                );
                ui.label(
                    egui::RichText::new(&commit.author)
                        .color(if dark {
                            egui::Color32::from_rgb(100, 220, 255)
                        } else {
                            egui::Color32::from_rgb(0, 130, 200)
                        })
                        .text_style(egui::TextStyle::Small),
                );
                ui.label(
                    egui::RichText::new(&commit.time)
                        .color(egui::Color32::GRAY)
                        .text_style(egui::TextStyle::Small),
                );
                ui.label(
                    egui::RichText::new(&commit.summary)
                        .color(ui.style().visuals.text_color()),
                );
            });
        }

        if commits.is_empty() {
            ui.label("No commits found");
        }
    });
}
