#![windows_subsystem = "windows"]

mod app;
mod git_ops;
mod recent;
mod ui;

use eframe::egui;

/// Included via build.rs — provides VERSION, GIT_HASH, GIT_DESCRIBE, BUILD_DATE constants.
mod version_info {
    include!(concat!(env!("OUT_DIR"), "/version_info.rs"));
}

fn native_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select a Git repository")
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

fn main() -> eframe::Result<()> {
    let app_title = format!("Git Manager v{}", version_info::VERSION);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 550.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title(&app_title),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "Git Manager",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new()))),
    )
}
