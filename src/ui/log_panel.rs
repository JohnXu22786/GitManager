use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Commit Log").heading()).truncate())
            .on_hover_text("Commit Log");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::add_enabled_ellipsis(ui, !app.is_busy(), "🔄 Refresh").clicked() {
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
                let sha_clone = commit.short_sha.clone();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&commit.short_sha)
                            .color(egui::Color32::from_rgb(180, 160, 100))
                            .monospace(),
                    )
                    .truncate(),
                )
                .on_hover_text(sha_clone);
                let author_clone = commit.author.clone();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&commit.author)
                            .color(egui::Color32::from_rgb(100, 200, 255))
                            .text_style(egui::TextStyle::Small),
                    )
                    .truncate(),
                )
                .on_hover_text(author_clone);
                let time_clone = commit.time.clone();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&commit.time)
                            .color(egui::Color32::GRAY)
                            .text_style(egui::TextStyle::Small),
                    )
                    .truncate(),
                )
                .on_hover_text(time_clone);
                let summary_clone = commit.summary.clone();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&commit.summary)
                            .color(ui.style().visuals.text_color()),
                    )
                    .truncate(),
                )
                .on_hover_text(summary_clone);

            });
        }

        if commits.is_empty() {
            ui.label("No commits found");
        }
    });
}
