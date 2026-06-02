use crate::app::App;
use eframe::egui;
use crate::git_ops::CommitInfo;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Commit Log");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🔄 Refresh").clicked() { app.refresh_all(); }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Search:");
        if ui.text_edit_singleline(&mut app.log_search).changed() {
            app.commits = app.git.log(100).unwrap_or_default();
        }
    });

    ui.separator();

    let filter = app.log_search.to_lowercase();
    let commits = app.commits.clone();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for commit in &commits {
            if !filter.is_empty()
                && !commit.message.to_lowercase().contains(&filter)
                && !commit.author.to_lowercase().contains(&filter)
                && !commit.short_sha.contains(&filter)
            {
                continue;
            }

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&commit.short_sha)
                        .color(egui::Color32::from_rgb(180, 160, 100))
                        .monospace(),
                );
                ui.label(
                    egui::RichText::new(&commit.author)
                        .color(egui::Color32::from_rgb(100, 200, 255))
                        .size(12.0),
                );
                ui.label(
                    egui::RichText::new(&commit.time)
                        .color(egui::Color32::GRAY)
                        .size(11.0),
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
