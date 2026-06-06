use crate::app::App;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Stash");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🔄 Refresh").clicked() {
                app.refresh_all();
            }
        });
    });

    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Message:");
        ui.text_edit_singleline(&mut app.stash_message);
    });
    if ui.button("Stash All").clicked() {
        let msg = if app.stash_message.trim().is_empty() {
            None
        } else {
            Some(app.stash_message.trim().to_string())
        };
        match app.git.stash_all(msg.as_deref()) {
            Ok(()) => {
                app.show_success("Stashed changes".into());
                app.stash_message.clear();
                app.refresh_all();
            }
            Err(e) => app.show_error(e),
        }
    }

    ui.separator();

    if ui.button("Pop Stash").clicked() {
        match app.git.stash_pop() {
            Ok(()) => {
                app.show_success("Stash popped".into());
                app.refresh_all();
            }
            Err(e) => app.show_error(e),
        }
    }
    if ui.button("Apply Stash").clicked() {
        match app.git.stash_apply() {
            Ok(()) => {
                app.show_success("Stash applied".into());
                app.refresh_all();
            }
            Err(e) => app.show_error(e),
        }
    }

    ui.separator();

    let stashes = app.stashes.clone();
    if !stashes.is_empty() {
        ui.label(egui::RichText::new(format!("Stash List ({} entries)", stashes.len())).strong());
        for stash in &stashes {
            let index = stash.index;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("stash@{{{}}}", stash.index))
                        .color(egui::Color32::from_rgb(200, 150, 100))
                        .monospace(),
                );
                ui.label(&stash.message);
                ui.label(
                    egui::RichText::new(&stash.time)
                        .color(egui::Color32::GRAY)
                        .text_style(egui::TextStyle::Small),
                );
                if ui.button("Drop").clicked() {
                    match app.git.stash_drop(index) {
                        Ok(()) => {
                            app.show_success(format!("Dropped stash@{{{}}}", index));
                            app.refresh_all();
                        }
                        Err(e) => app.show_error(e),
                    }
                }
                if ui.button("Apply").clicked() {
                    match app.git.stash_apply() {
                        Ok(()) => {
                            app.show_success("Stash applied".into());
                            app.refresh_all();
                        }
                        Err(e) => app.show_error(e),
                    }
                }
            });
        }
    } else {
        ui.label("No stashes");
    }
}
