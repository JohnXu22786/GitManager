#![windows_subsystem = "windows"]

mod app;
mod git_ops;
mod ui;
mod updater;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("Git Manager"),
        ..Default::default()
    };

    eframe::run_native(
        "Git Manager",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new()))),
    )
}
